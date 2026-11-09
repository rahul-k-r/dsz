using System.Net;
using System.Net.Sockets;
using System.Text.Json;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;

namespace DszBattery;

internal sealed partial class ProfilesPage : ListPage
{
    private string? error;

    public ProfilesPage()
    {
        Id = "dsz.controller-profiles";
        Title = "DSZ Profiles";
        Name = "View and switch profiles";
    }

    public override IListItem[] GetItems()
    {
        var state = ProfileConnection.Send();
        if (state is null)
            return [new ListItem(new NoOpCommand()) { Title = "Start a DSZ build with profile support" }];

        return Items(state, error, SwitchProfile);
    }

    internal static IListItem[] Items(ProfileState state, string? error, Action<string> select)
    {
        var active = string.Join(", ", state.active.Distinct(StringComparer.Ordinal));
        var status = new ListItem(new NoOpCommand()) { Title = $"Active: {active}", Subtitle = error ?? "" };
        return [status, .. state.profiles.Select(profile =>
            new ListItem(new SelectProfile(profile, select))
            {
                Title = profile,
                Subtitle = state.active.Contains(profile, StringComparer.Ordinal) ? "Active" : "Switch profile",
            })];
    }

    private void SwitchProfile(string profile)
    {
        error = ProfileConnection.Send(profile)?.applied == true ? null : "DSZ could not switch profiles";
        RaiseItemsChanged();
    }
}

internal sealed partial class SelectProfile(string profile, Action<string> select) : InvokableCommand
{
    public override ICommandResult Invoke()
    {
        select(profile);
        return CommandResult.KeepOpen();
    }
}

internal sealed record ProfileState(string[] active, string[] profiles, bool applied);

internal static class ProfileConnection
{
    internal static ProfileState? Send(string? profile = null)
    {
        var port = Battery.TrayProperty("DSZ.CmdPal.Port");
        var first = (nuint)Battery.TrayProperty("DSZ.CmdPal.Key1");
        var second = (nuint)Battery.TrayProperty("DSZ.CmdPal.Key2");
        if (port is <= 0 or > 65535 || first == 0 || second == 0)
            return null;

        try
        {
            using var client = new TcpClient(AddressFamily.InterNetwork);
            using var deadline = new CancellationTokenSource(TimeSpan.FromMilliseconds(500));
            client.ConnectAsync(IPAddress.Loopback, (int)port, deadline.Token).AsTask().GetAwaiter().GetResult();
            using var stream = client.GetStream();
            var bytes = JsonSerializer.SerializeToUtf8Bytes(new { token = $"{first:x}:{second:x}", profile });
            stream.WriteAsync(bytes, deadline.Token).AsTask().GetAwaiter().GetResult();
            stream.WriteByte((byte)'\n');
            using var reader = new StreamReader(stream);
            var line = reader.ReadLineAsync(deadline.Token).AsTask().GetAwaiter().GetResult();
            return line is null ? null : JsonSerializer.Deserialize<ProfileState>(line);
        }
        catch (Exception exception) when (exception is IOException or SocketException or OperationCanceledException or JsonException)
        {
            return null;
        }
    }
}
