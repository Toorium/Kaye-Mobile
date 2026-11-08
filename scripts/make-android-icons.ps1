# Draws Kaye's Android launcher icons from assets\logo_mark.png: the white mark on coral
# #F36B70, as on the computer. Run it again only if the logo changes.
#
#   powershell -ExecutionPolicy Bypass -File scripts\make-android-icons.ps1

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$project = Split-Path -Parent $PSScriptRoot
$mark = [System.Drawing.Image]::FromFile((Join-Path $project 'assets\logo_mark.png'))
$res = Join-Path $project 'android\res'
$coral = [System.Drawing.Color]::FromArgb(255, 0xF3, 0x6B, 0x70)

function Save-Icon([int]$size, [string]$path, [bool]$background, [double]$markShare) {
    $bitmap = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.SmoothingMode = 'AntiAlias'
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.Clear([System.Drawing.Color]::Transparent)
    if ($background) {
        # The same corners as the computer's logo: 56 on 256.
        $r = [int]($size * 56 / 256)
        $shape = New-Object System.Drawing.Drawing2D.GraphicsPath
        $shape.AddArc(0, 0, 2 * $r, 2 * $r, 180, 90)
        $shape.AddArc($size - 2 * $r - 1, 0, 2 * $r, 2 * $r, 270, 90)
        $shape.AddArc($size - 2 * $r - 1, $size - 2 * $r - 1, 2 * $r, 2 * $r, 0, 90)
        $shape.AddArc(0, $size - 2 * $r - 1, 2 * $r, 2 * $r, 90, 90)
        $shape.CloseFigure()
        $g.FillPath((New-Object System.Drawing.SolidBrush $coral), $shape)
    }
    $side = [int]($size * $markShare)
    $at = [int](($size - $side) / 2)
    $g.DrawImage($mark, $at, $at, $side, $side)
    $g.Dispose()
    New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}

$densities = @{ 'mdpi' = 1.0; 'hdpi' = 1.5; 'xhdpi' = 2.0; 'xxhdpi' = 3.0; 'xxxhdpi' = 4.0 }
foreach ($name in $densities.Keys) {
    $scale = $densities[$name]
    # Older launchers: the whole rounded square, 48 dp, with the computer's 36 on 256 margin.
    Save-Icon ([int](48 * $scale)) (Join-Path $res "mipmap-$name\ic_launcher.png") $true (1 - 2 * 36 / 256)
    # Newer launchers cut their own shape from a 108 dp layer, and only the middle 66 dp is
    # sure to show.
    Save-Icon ([int](108 * $scale)) (Join-Path $res "mipmap-$name\ic_launcher_foreground.png") $false (66 / 108 * (1 - 2 * 36 / 256))
}
$mark.Dispose()

New-Item -ItemType Directory -Force (Join-Path $res 'mipmap-anydpi-v26'), (Join-Path $res 'values') | Out-Null
$adaptive = @'
<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
</adaptive-icon>
'@
[IO.File]::WriteAllText((Join-Path $res 'mipmap-anydpi-v26\ic_launcher.xml'), $adaptive)
$colors = @'
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <color name="ic_launcher_background">#F36B70</color>
</resources>
'@
[IO.File]::WriteAllText((Join-Path $res 'values\colors.xml'), $colors)
Write-Host "Icons written to $res"
