namespace SHLauncher.Infrastructure.Mocks;

public enum MockScenario { Success, LaunchFailure, Cancellation, UnexpectedException, SlowLaunch, RuntimeMissing, InstanceInvalid }

public sealed record MockLaunchOptions
{
    public MockScenario Scenario { get; init; } = MockScenario.Success;
    public TimeSpan StepDelay { get; init; } = TimeSpan.FromMilliseconds(450);
    public TimeSpan RunningDuration { get; init; } = TimeSpan.FromSeconds(8);
    public int FailureAttempts { get; init; } = 1;
}
