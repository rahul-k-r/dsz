using System.Collections.Concurrent;
using Microsoft.CommandPalette.Extensions;
using Microsoft.CommandPalette.Extensions.Toolkit;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace DszBattery;

internal static class ProfileIcons
{
    private static readonly ConcurrentDictionary<int, IconInfo> Icons = new();

    internal static IIconInfo? For(ProfileState state, string profile)
    {
        if (state.colors?.GetValueOrDefault(profile) is not [var red, var green, var blue])
            return null;
        return Icons.GetOrAdd((red << 16) | (green << 8) | blue, Create);
    }

    private static IconInfo Create(int rgb)
    {
        const int size = 24;
        var pixels = new byte[size * size * 4];
        for (int y = 0; y < size; y++)
        {
            for (int x = 0; x < size; x++)
            {
                var distance = Math.Sqrt(Math.Pow(x - 11.5, 2) + Math.Pow(y - 11.5, 2));
                var index = (y * size + x) * 4;
                pixels[index] = (byte)rgb;
                pixels[index + 1] = (byte)(rgb >> 8);
                pixels[index + 2] = (byte)(rgb >> 16);
                pixels[index + 3] = (byte)(Math.Clamp(10.5 - distance, 0, 1) * 255);
            }
        }
        var stream = new InMemoryRandomAccessStream();
        var encoder = BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream).AsTask().GetAwaiter().GetResult();
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Straight, size, size, 96, 96, pixels);
        encoder.FlushAsync().AsTask().GetAwaiter().GetResult();
        stream.Seek(0);
        var data = new IconData(RandomAccessStreamReference.CreateFromStream(stream));
        return new IconInfo(data, data);
    }
}
