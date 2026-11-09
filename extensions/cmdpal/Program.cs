using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;
using Shmuelie.WinRTServer.CsWinRT;

namespace DszBattery;

internal static class Program
{
    [MTAThread]
    private static int Main(string[] args)
    {
        if (args is ["--self-test"])
        {
            using var provider = new BatteryProvider(startTimer: false);
            var band = provider.GetDockBands().Single();
            var dockPage = (IListPage)band.Command;
            provider.Refresh(82);
            var showsCharge = dockPage.GetItems()[0].Title == "80%";
            provider.Refresh(1);
            var clearsCharge = dockPage.GetItems()[0].Title == "—";
            var state = new ProfileState(["Default"], ["Default", "Racing"], true,
                new() { ["Racing"] = [34, 128, 200] },
                [new(42, "DualSense", 80, false, "Default", true, false)]);
            provider.UpdateProfiles(state);
            var commands = provider.TopLevelCommands();
            var batteryStatus = commands.Single(command => command.Title == "DSZ Battery");
            string? selected = null;
            var profiles = ProfilesPage.Items(state,
                null, profile => selected = profile);
            ((IInvokableCommand)profiles[2].Command).Invoke(profiles[2]);
            var device = ControllersPage.Items(state, (_, _) => CommandResult.KeepOpen()).Single();
            var shortcut = commands.Single(command => command.Title == "DSZ Racing");

            return Battery.Label(0) == "DSZ battery unavailable"
                && Battery.Label(1) == "No controller connected"
                && Battery.Label(2) == "0%"
                && Battery.Label(82) == "80%"
                && Battery.Label(102) == "100%"
                && Battery.Label(103) == "DSZ battery unavailable"
                && IsComLaunch(["-RegisterProcessAsComServer", "-Embedding"])
                && band.Command.Id == "dsz.controller-battery.dock"
                && showsCharge && clearsCharge
                && batteryStatus.Command is NoOpCommand
                && batteryStatus.Subtitle == "No controller connected"
                && profiles[2].Icon?.Light?.Data is not null
                && shortcut.Icon?.Light?.Data is not null
                && provider.GetCommandItem(shortcut.Command.Id) == shortcut
                && dockPage.GetItems()[0].Command.Id == "dsz.controllers"
                && dockPage.GetItems()[1].Command.Id == "dsz.controller-profiles"
                && device.Subtitle == "80% · USB · Default"
                && device.MoreCommands.Length == 1
                && profiles[0].Title == "Active: Default" && profiles[1].Subtitle == "Active"
                && selected == "Racing"
                && !IsComLaunch(["-Embedding"]) ? 0 : 1;
        }

        if (!IsComLaunch(args))
            return 0;

        using var stopped = new ManualResetEvent(false);
        var extension = new Extension(stopped);
        var server = new Shmuelie.WinRTServer.ComServer();
        server.RegisterClass<Extension, IExtension>(() => extension);
        server.Start();
        stopped.WaitOne();
        server.Stop();
        server.UnsafeDispose();
        return 0;
    }

    // Packaged COM activation can append -Embedding to the manifest arguments.
    private static bool IsComLaunch(string[] args) =>
        args.Contains("-RegisterProcessAsComServer", StringComparer.OrdinalIgnoreCase);
}

[Guid("614E15A3-096F-4215-AB62-281AB86B08C0")]
public sealed partial class Extension(ManualResetEvent stopped) : IExtension, IDisposable
{
    private readonly BatteryProvider provider = new();

    public object? GetProvider(ProviderType type) => type == ProviderType.Commands ? provider : null;

    public void Dispose()
    {
        provider.Dispose();
        stopped.Set();
    }
}

internal sealed partial class BatteryProvider : CommandProvider, IDisposable
{
    private readonly CommandItem item;
    private readonly ListItem dockStatus;
    private readonly ListItem dockProfile;
    private readonly WrappedDockItem dockBand;
    private readonly Timer? timer;
    private readonly ProfilesPage profiles = new();
    private readonly ControllersPage controllers = new();
    private readonly CommandItem profilesItem;
    private readonly CommandItem controllersItem;
    private volatile ICommandItem[] shortcuts = [];
    private string? profileSignature;
    private int refreshing;

    public BatteryProvider(bool startTimer = true)
    {
        Id = "dsz.controller-battery";
        DisplayName = "DSZ Battery";
        Icon = IconHelpers.FromRelativePath("Assets\\Square44.png");
        item = new CommandItem(new NoOpCommand { Id = "dsz.controller-battery.readout" })
            { Title = DisplayName, Icon = Icon };
        dockStatus = new ListItem(controllers) { Icon = Icon };
        dockProfile = new ListItem(profiles) { Title = "Profiles", Icon = Icon };
        dockBand = new WrappedDockItem([dockStatus, dockProfile], "dsz.controller-battery.dock", DisplayName);
        profilesItem = new CommandItem(profiles) { Title = profiles.Title, Icon = Icon };
        controllersItem = new CommandItem(controllers) { Title = controllers.Title, Icon = Icon };
        Refresh(Battery.Read());
        if (startTimer)
            timer = new Timer(_ => RefreshFromDsz(), null, TimeSpan.Zero, TimeSpan.FromSeconds(1));
    }

    internal void Refresh(nint value)
    {
        var label = Battery.Label(value);
        if (item.Subtitle == label)
            return;
        item.Subtitle = label;
        // Compact Dock hides subtitles, so keep the charge in the title.
        dockStatus.Title = value is >= 2 and <= 102 ? label : "—";
        dockStatus.Subtitle = value is >= 2 and <= 102 ? DisplayName : label;
    }

    private void RefreshFromDsz()
    {
        if (Interlocked.Exchange(ref refreshing, 1) != 0)
            return;
        try
        {
            Refresh(Battery.Read());
            UpdateProfiles(ProfileConnection.Send());
        }
        finally
        {
            Volatile.Write(ref refreshing, 0);
        }
    }

    internal void UpdateProfiles(ProfileState? state)
    {
        var active = state?.active.Distinct(StringComparer.Ordinal).ToArray() ?? [];
        var title = active is [var name] ? name : "Profiles";
        if (dockProfile.Title != title)
            dockProfile.Title = title;
        var subtitle = active.Length == 0 ? "DSZ profiles" : string.Join(", ", active);
        if (dockProfile.Subtitle != subtitle)
            dockProfile.Subtitle = subtitle;
        var icon = active is [var profile] ? ProfileIcons.For(state!, profile) ?? Icon : Icon;
        if (dockProfile.Icon != icon)
            dockProfile.Icon = icon;

        var signature = state is null ? "" : JsonSerializer.Serialize(new { state.profiles, state.colors });
        if (profileSignature == signature)
            return;
        profileSignature = signature;
        shortcuts = state?.profiles.Select(profile => (ICommandItem)new CommandItem(
            new ControllerAction(() => ProfileConnection.Send(profile)?.applied == true
                ? CommandResult.Dismiss() : CommandResult.ShowToast("DSZ could not switch profiles"))
            { Id = $"dsz.profile.{Convert.ToHexString(Encoding.UTF8.GetBytes(profile))}" })
            { Title = $"DSZ {profile}", Icon = ProfileIcons.For(state, profile) ?? Icon }).ToArray() ?? [];
        RaiseItemsChanged();
    }

    public override ICommandItem[] TopLevelCommands() => [item, profilesItem, controllersItem, .. shortcuts];
    public override ICommandItem? GetCommandItem(string id) =>
        shortcuts.FirstOrDefault(command => command.Command.Id == id) ?? base.GetCommandItem(id);
    public override ICommandItem[] GetDockBands() => [dockBand];
    public override void Dispose()
    {
        timer?.Dispose();
        base.Dispose();
    }
}

internal static partial class Battery
{
    internal static nint TrayProperty(string name)
    {
        var window = FindWindow("DSZTray", "DSZTray");
        return window == 0 ? 0 : GetProp(window, name);
    }

    internal static nint Read()
        => TrayProperty("DSZ.BatteryPercent.v1");

    internal static string Label(nint value) => value switch
    {
        1 => "No controller connected",
        >= 2 and <= 102 => $"{value - 2}%",
        _ => "DSZ battery unavailable",
    };

    [LibraryImport("user32.dll", EntryPoint = "FindWindowW", StringMarshalling = StringMarshalling.Utf16)]
    private static partial nint FindWindow(string? className, string windowName);

    [LibraryImport("user32.dll", EntryPoint = "GetPropW", StringMarshalling = StringMarshalling.Utf16)]
    private static partial nint GetProp(nint window, string name);
}
