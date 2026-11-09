using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace DszBattery;

internal sealed partial class ControllersPage : ListPage
{
    public ControllersPage()
    {
        Id = "dsz.controllers";
        Title = "DSZ Controllers";
        Name = "View controllers";
    }

    public override IListItem[] GetItems()
    {
        var state = ProfileConnection.Send();
        if (state?.controllers is null)
            return [new ListItem(new NoOpCommand()) { Title = "Start a DSZ build with controller controls" }];
        if (state.controllers.Length == 0)
            return [new ListItem(new NoOpCommand()) { Title = "No controller connected" }];

        return Items(state, RunAction);
    }

    internal static IListItem[] Items(ProfileState state, Func<ulong, string, ICommandResult> run) =>
        (state.controllers ?? []).Select(controller =>
        {
            var actions = new List<IContextItem>();
            if (controller.identify)
                actions.Add(new CommandContextItem(new ControllerAction(() => run(controller.id, "identify")))
                    { Title = "Identify controller" });
            if (controller.powerOff)
                actions.Add(new CommandContextItem(new ControllerAction(() => run(controller.id, "power_off")))
                    { Title = "Power off Bluetooth controller" });
            return (IListItem)new ListItem(new ProfilesPage(controller.id))
            {
                Title = controller.name,
                Subtitle = $"{controller.battery}% · {(controller.bluetooth ? "Bluetooth" : "USB")} · {controller.profile}",
                Icon = ProfileIcons.For(state, controller.profile),
                MoreCommands = [.. actions],
            };
        }).ToArray();

    private ICommandResult RunAction(ulong id, string action)
    {
        var accepted = ProfileConnection.Send(device: id, action: action)?.applied == true;
        RaiseItemsChanged();
        return accepted ? CommandResult.KeepOpen() : CommandResult.ShowToast("DSZ could not complete that action");
    }
}

internal sealed partial class ControllerAction(Func<ICommandResult> run) : InvokableCommand
{
    public override ICommandResult Invoke() => run();
}
