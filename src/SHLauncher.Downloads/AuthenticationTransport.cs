using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Downloads;
public static class AuthenticationUrlPolicy
{
    public static Uri Parse(string value)
    {
        if(value.Length>2048||!Uri.TryCreate(value,UriKind.Absolute,out var uri)||uri.Scheme!="https"||uri.UserInfo.Length!=0||uri.Query.Length!=0||uri.Fragment.Length!=0||uri.Host.EndsWith(".local",StringComparison.OrdinalIgnoreCase)||uri.Host.Equals("localhost",StringComparison.OrdinalIgnoreCase))
            throw new AccountException(AccountError.InvalidInput,"认证服务器地址无效，请填写公网 HTTPS API 地址。");
        if(IPAddress.TryParse(uri.Host.Trim('[',']'),out var address)&&!IsPublic(address))throw new AccountException(AccountError.InvalidInput,"认证服务器不能指向本机或内网地址。");
        return new Uri(uri.AbsoluteUri.TrimEnd('/')+"/");
    }
    public static bool IsPublic(IPAddress address)
    {
        if(address.IsIPv4MappedToIPv6)address=address.MapToIPv4();var b=address.GetAddressBytes();
        if(b.Length==16)return b[0]>=0x20&&b[0]<=0x3f&&!(b[0]==0x20&&b[1]==0x02)&&!(b[0]==0x20&&b[1]==1&&((b[2]==0x0d&&b[3]==0xb8)||(b[2]==0&&b[3]==0)));
        return !(b[0] is 0 or 10 or 127||b[0]>=224||b[0]==100&&b[1] is >=64 and <=127||b[0]==169&&b[1]==254||b[0]==172&&b[1] is >=16 and <=31||b[0]==192&&(b[1]==168||b[1]==0)||b[0]==198&&(b[1] is 18 or 19||b[1]==51&&b[2]==100)||b[0]==203&&b[1]==0&&b[2]==113);
    }
}
// Password/session traffic is deliberately separate from artifact GET transport, caches and journals.
public sealed class AuthenticationTransport:IAuthenticationTransport,IDisposable
{
    private readonly HttpClient _client;
    public AuthenticationTransport()
    {
        var handler=new SocketsHttpHandler{AllowAutoRedirect=false,UseCookies=false,UseProxy=false,ConnectTimeout=TimeSpan.FromSeconds(5)};
        handler.ConnectCallback=async(context,token)=>
        {
            var addresses=await Dns.GetHostAddressesAsync(context.DnsEndPoint.Host,token).ConfigureAwait(false);
            if(addresses.Length==0||addresses.Any(a=>!AuthenticationUrlPolicy.IsPublic(a)))throw new AccountException(AccountError.InvalidInput,"认证服务器解析到了不可用的本机或内网地址。");
            foreach(var address in addresses)
            {
                var socket=new Socket(address.AddressFamily,SocketType.Stream,ProtocolType.Tcp);
                try{await socket.ConnectAsync(new IPEndPoint(address,context.DnsEndPoint.Port),token).ConfigureAwait(false);return new NetworkStream(socket,true);}
                catch(SocketException){socket.Dispose();}
                catch{socket.Dispose();throw;}
            }
            throw new AccountException(AccountError.Network,"认证服务器暂时无法连接。");
        };
        _client=new(handler){Timeout=Timeout.InfiniteTimeSpan};
    }
    public async Task<AuthenticationResponse> SendAsync(Uri uri,string method,ReadOnlyMemory<byte>? body,CancellationToken token)
    {
        AuthenticationUrlPolicy.Parse(uri.AbsoluteUri);
        if(method is not ("GET" or "POST")||body?.Length>32768)throw new AccountException(AccountError.InvalidInput,"认证请求无效。");
        byte[]? payload=body?.ToArray();
        try
        {
            using var timeout=CancellationTokenSource.CreateLinkedTokenSource(token);timeout.CancelAfter(TimeSpan.FromSeconds(15));
            using var request=new HttpRequestMessage(new HttpMethod(method),uri);
            request.Headers.Accept.ParseAdd("application/json");request.Headers.UserAgent.ParseAdd("SHLN/0.4");
            if(payload is not null){request.Content=new ByteArrayContent(payload);request.Content.Headers.ContentType=new("application/json");}
            using var response=await _client.SendAsync(request,HttpCompletionOption.ResponseHeadersRead,timeout.Token).ConfigureAwait(false);
            const int limit=256*1024;
            if(response.Content.Headers.ContentLength>limit||response.Content.Headers.ContentEncoding.Count!=0)throw new AccountException(AccountError.InvalidMetadata,"认证服务器响应超过上限或格式不受支持。");
            using var output=new MemoryStream();await using var input=await response.Content.ReadAsStreamAsync(timeout.Token).ConfigureAwait(false);
            var buffer=new byte[8192];int read;
            while((read=await input.ReadAsync(buffer,timeout.Token).ConfigureAwait(false))>0)
            {if(output.Length+read>limit)throw new AccountException(AccountError.InvalidMetadata,"认证服务器响应超过安全大小上限。");await output.WriteAsync(buffer.AsMemory(0,read),timeout.Token).ConfigureAwait(false);}
            var location=response.Headers.TryGetValues("X-Authlib-Injector-API-Location",out var values)?values.SingleOrDefault():null;
            return new((int)response.StatusCode,output.ToArray(),location);
        }
        catch(OperationCanceledException) when(!token.IsCancellationRequested){throw new AccountException(AccountError.Network,"认证服务器连接超时，请稍后重试。");}
        catch(Exception error) when(error is HttpRequestException or IOException or SocketException){throw new AccountException(AccountError.Network,"认证服务器暂时无法连接，请检查网络后重试。");}
        finally{if(payload is not null)CryptographicOperations.ZeroMemory(payload);}
    }
    public void Dispose()=>_client.Dispose();
}
