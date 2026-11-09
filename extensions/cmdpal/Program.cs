using System.Runtime.InteropServices;
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
            using var provider = new BatteryProvider();
            var band = provider.GetDockBands().Single();
            var dockPage = (IListPage)band.Command;
            provider.Refresh(82);
            var showsCharge = dockPage.GetItems().Single().Title == "80%";
            provider.Refresh(1);
            var clearsCharge = dockPage.GetItems().Single().Title == "—";
            string? selected = null;
            var profiles = ProfilesPage.Items(new ProfileState(["Default"], ["Default", "Racing"], true),
                null, profile => selected = profile);
            ((IInvokableCommand)profiles[2].Command).Invoke(profiles[2]);

            return Battery.Label(0) == "DSZ battery unavailable"
                && Battery.Label(1) == "No controller connected"
                && Battery.Label(2) == "0%"
                && Battery.Label(82) == "80%"
                && Battery.Label(102) == "100%"
                && Battery.Label(103) == "DSZ battery unavailable"
                && IsComLaunch(["-RegisterProcessAsComServer", "-Embedding"])
                && band.Command.Id == "dsz.controller-battery.dock"
                && showsCharge && clearsCharge
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
    private readonly WrappedDockItem dockBand;
    private readonly Timer timer;
    private readonly BatteryPage page = new();
    private readonly ProfilesPage profiles = new();
    private readonly CommandItem profilesItem;

    public BatteryProvider()
    {
        Id = "dsz.controller-battery";
        DisplayName = "DSZ Battery";
        Icon = new IconInfo("\uE850");
        item = new CommandItem(page) { Title = DisplayName, Icon = Icon };
        dockStatus = new ListItem(new NoOpCommand()) { Icon = Icon };
        dockBand = new WrappedDockItem([dockStatus], "dsz.controller-battery.dock", DisplayName);
        profilesItem = new CommandItem(profiles) { Title = profiles.Title };
        Refresh(Battery.Read());
        timer = new Timer(_ => Refresh(Battery.Read()), null, TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(1));
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
        page.Refresh(label);
    }

    public override ICommandItem[] TopLevelCommands() => [item, profilesItem];
    public override ICommandItem[] GetDockBands() => [dockBand];
    public override void Dispose()
    {
        timer.Dispose();
        base.Dispose();
    }
}

internal sealed partial class BatteryPage : ListPage
{
    private readonly ListItem status = new(new NoOpCommand());

    public BatteryPage()
    {
        Id = "dsz.controller-battery.status";
        Title = "DSZ Battery";
        Name = "View battery";
    }

    public void Refresh(string label)
    {
        status.Title = label;
        RaiseItemsChanged();
    }

    public override IListItem[] GetItems() => [status];
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
