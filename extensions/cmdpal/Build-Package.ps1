param(
    [Parameter(Mandatory)] [string] $DszExe,
    [Parameter(Mandatory)] [string] $MakeAppx,
    [Parameter(Mandatory)] [string] $Output
)

$ErrorActionPreference = 'Stop'
$DszExe = (Resolve-Path -LiteralPath $DszExe).Path
$Output = [IO.Path]::GetFullPath($Output)
$stage = Join-Path $Output ([Guid]::NewGuid().ToString('N'))
$artifacts = Join-Path $PSScriptRoot '../../target/cmdpal-build'

dotnet publish (Join-Path $PSScriptRoot 'DszBattery.csproj') -c Release -r win-x64 `
    --self-contained --artifacts-path $artifacts -o $stage
if ($LASTEXITCODE -ne 0) { throw 'Extension build failed.' }
& (Join-Path $stage 'DszBattery.exe') --self-test | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Extension battery display test failed.' }

Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'AppxManifest.xml') -Destination $stage
$assets = New-Item -ItemType Directory -Path (Join-Path $stage 'Assets')
$public = New-Item -ItemType Directory -Path (Join-Path $stage 'Public')
Add-Type -AssemblyName System.Drawing
$icon = [Drawing.Icon]::ExtractAssociatedIcon($DszExe)
try {
    foreach ($logo in @(@('Square44.png', 44), @('Square150.png', 150), @('Store.png', 50))) {
        $image = [Drawing.Bitmap]::new([int]$logo[1], [int]$logo[1])
        $graphics = [Drawing.Graphics]::FromImage($image)
        try {
            $graphics.DrawIcon($icon, [Drawing.Rectangle]::new(0, 0, $image.Width, $image.Height))
            $image.Save((Join-Path $assets.FullName $logo[0]), [Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $graphics.Dispose()
            $image.Dispose()
        }
    }
} finally {
    $icon.Dispose()
}
Copy-Item -LiteralPath (Join-Path $assets.FullName 'Store.png') -Destination $public.FullName

& $MakeAppx pack /d $stage /p (Join-Path $Output 'DszBattery-x64.msix') /o
if ($LASTEXITCODE -ne 0) { throw 'MSIX packaging failed.' }

# The loose package preserves the same identity as local developer installs.
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $Output 'DszBattery-x64.zip') -Force
