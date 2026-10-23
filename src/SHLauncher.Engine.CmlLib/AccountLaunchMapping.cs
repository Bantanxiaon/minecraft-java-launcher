using System.Security.Cryptography;
using CmlLib.Core.Auth;
using CmlLib.Core.ProcessBuilder;
using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Engine.CmlLib;
public static class AccountLaunchMapping
{
    public static async Task ApplyAsync(MLaunchOption option,LaunchIdentity identity,IAuthenticationAgentService? agents,CancellationToken token)
    {
        option.Session=new MSession{Username=identity.Name,UUID=identity.ProfileId.ToString("N"),AccessToken=identity.AccessToken,UserType=identity.UserType};
        option.UserProperties=identity.Properties;
        if(identity.Authority is null)return;
        if(agents is null)throw new AccountException(AccountError.Unavailable,"此启动环境尚未接入第三方认证组件。");
        var agent=await agents.PrepareAsync(token).ConfigureAwait(false);
        if(!Path.IsPathFullyQualified(agent.Path)||agent.Sha256.Length!=64)throw new AccountException(AccountError.Unavailable,"认证启动组件记录无效。");
        await using var file=File.OpenRead(agent.Path);
        if(!Convert.ToHexString(await SHA256.HashDataAsync(file,token).ConfigureAwait(false)).Equals(agent.Sha256,StringComparison.OrdinalIgnoreCase))throw new AccountException(AccountError.Unavailable,"认证启动组件校验失败，请重新准备。");
        option.ExtraJvmArguments=option.ExtraJvmArguments.Concat([new MArgument("-javaagent:"+agent.Path+"="+identity.Authority.AbsoluteUri),new MArgument("-Dauthlibinjector.noLogFile")]).ToArray();
    }
}
