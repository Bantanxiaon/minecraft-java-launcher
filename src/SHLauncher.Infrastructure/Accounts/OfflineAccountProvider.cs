using SHLauncher.Contracts.Accounts;
namespace SHLauncher.Infrastructure.Accounts;
public sealed class OfflineAccountProvider : IAccountProvider
{
    private readonly LaunchIdentity _account = new("SHTestPlayer", SHLauncher.Core.Accounts.OfflineUuid.ForName("SHTestPlayer"));
    public Task<LaunchIdentity> GetAsync(CancellationToken token) { token.ThrowIfCancellationRequested(); return Task.FromResult(_account); }
}
