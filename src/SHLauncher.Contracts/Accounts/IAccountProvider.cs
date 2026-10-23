namespace SHLauncher.Contracts.Accounts;
// Ephemeral launch identity. Never log or persist a live identity.
public sealed class LaunchIdentity(string name,Guid profileId,string accessToken="0",string userType="legacy",string properties="{}",Uri? authority=null)
{
    public string Name {get;}=name;
    public Guid ProfileId {get;}=profileId;
    [System.Text.Json.Serialization.JsonIgnore] public string AccessToken {get;}=accessToken;
    public string UserType {get;}=userType;
    public string Properties {get;}=properties;
    public Uri? Authority {get;}=authority;
    public override string ToString()=>"Launch identity [credentials hidden]";
}
public interface IAccountProvider { Task<LaunchIdentity> GetAsync(CancellationToken token); }
