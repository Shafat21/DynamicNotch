# Regenerates the NSIS installer artwork (sidebar 164x314, header 150x57) from the app icon. Run: powershell -File scripts/make-installer-images.ps1
Add-Type -AssemblyName System.Drawing
$root = Split-Path $PSScriptRoot -Parent
$icon = [System.Drawing.Image]::FromFile("$root\src-tauri\icons\128x128.png")
$out = "$root\src-tauri\windows"

function New-Canvas($w, $h) {
  $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = 'AntiAlias'; $g.InterpolationMode = 'HighQualityBicubic'; $g.TextRenderingHint = 'AntiAliasGridFit'
  return @($bmp, $g)
}

# sidebar: the island's dark blue gradient, glow, icon, name
$bmp, $g = New-Canvas 164 314
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.Rectangle]::new(0,0,164,314)), ([System.Drawing.Color]::FromArgb(7,12,26)), ([System.Drawing.Color]::FromArgb(14,42,96)), 90
$g.FillRectangle($grad, 0, 0, 164, 314)
$glow = New-Object System.Drawing.Drawing2D.GraphicsPath; $glow.AddEllipse(-40, 150, 244, 244)
$pb = New-Object System.Drawing.Drawing2D.PathGradientBrush $glow
$pb.CenterColor = [System.Drawing.Color]::FromArgb(90, 40, 120, 255); $pb.SurroundColors = @([System.Drawing.Color]::FromArgb(0, 40, 120, 255))
$g.FillPath($pb, $glow)
# a notch pill, like the island
$pill = New-Object System.Drawing.Drawing2D.GraphicsPath
$pill.AddArc(32, 22, 20, 20, 90, 180); $pill.AddArc(112, 22, 20, 20, 270, 180); $pill.CloseFigure()
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(0,0,0))), $pill)
$g.DrawPath((New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(60,255,255,255)), 1), $pill)
$g.DrawImage($icon, 36, 92, 92, 92)
$f1 = New-Object System.Drawing.Font 'Segoe UI Semibold', 13
$f2 = New-Object System.Drawing.Font 'Segoe UI', 8.5
$c = New-Object System.Drawing.StringFormat; $c.Alignment = 'Center'
$g.DrawString('Dynamic Notch', $f1, [System.Drawing.Brushes]::White, [System.Drawing.RectangleF]::new(0, 200, 164, 24), $c)
$g.DrawString('Your island for the notch', $f2, (New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(170,200,220,255))), [System.Drawing.RectangleF]::new(0, 226, 164, 20), $c)
$bmp.Save("$out\sidebar.bmp", [System.Drawing.Imaging.ImageFormat]::Bmp); $g.Dispose(); $bmp.Dispose()

# header: dark chip with the icon, sits on the white header band
$bmp, $g = New-Canvas 150 57
$g.Clear([System.Drawing.Color]::White)
$chip = New-Object System.Drawing.Drawing2D.GraphicsPath
$chip.AddArc(86, 6, 14, 14, 180, 90); $chip.AddArc(134, 6, 14, 14, 270, 90); $chip.AddArc(134, 37, 14, 14, 0, 90); $chip.AddArc(86, 37, 14, 14, 90, 90); $chip.CloseFigure()
$g.FillPath((New-Object System.Drawing.Drawing2D.LinearGradientBrush ([System.Drawing.Rectangle]::new(86,6,62,45)), ([System.Drawing.Color]::FromArgb(7,12,26)), ([System.Drawing.Color]::FromArgb(14,42,96)), 90), $chip)
$g.DrawImage($icon, 97, 11, 40, 40)
$bmp.Save("$out\header.bmp", [System.Drawing.Imaging.ImageFormat]::Bmp); $g.Dispose(); $bmp.Dispose()
$icon.Dispose()
