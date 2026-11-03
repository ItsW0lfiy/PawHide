#requires -Version 7
# Restore the opaque original Hidden Paw asset without regenerating artwork.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Source,
    [string]$Destination = (Join-Path $PSScriptRoot '../ui/assets'),
    [string]$Comparison
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$original = [Drawing.Bitmap]::new((Resolve-Path -LiteralPath $Source).Path)
if ($original.Width -ne 256 -or $original.Height -ne 256) { throw 'Expected the original 256 x 256 master.' }
$clean = [Drawing.Bitmap]::new(256, 256, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
try {
    for ($y = 0; $y -lt 256; $y++) {
        for ($x = 0; $x -lt 256; $x++) {
            $pixel = $original.GetPixel($x, $y)
            # The original badge outline is a rounded square. Touch only its
            # outer matte and glow; every pixel inside the outline is retained.
            $qx = [Math]::Abs($x - 127.5) - 60.0
            $qy = [Math]::Abs($y - 127.5) - 60.0
            $distance = [Math]::Sqrt([Math]::Pow([Math]::Max($qx, 0), 2) + [Math]::Pow([Math]::Max($qy, 0), 2)) + [Math]::Min([Math]::Max($qx, $qy), 0) - 55.0
            $t = [Math]::Clamp($distance / 8.0, 0.0, 1.0)
            $alpha = 1.0 - $t * $t * (3.0 - 2.0 * $t)
            $a = [int][Math]::Round(255 * $alpha)
            if ($a -eq 255) { $clean.SetPixel($x, $y, $pixel); continue }
            if ($a -eq 0) { $clean.SetPixel($x, $y, [Drawing.Color]::Transparent); continue }
            # Remove the known charcoal matte from partially transparent glow.
            $r = [int][Math]::Clamp([Math]::Round(($pixel.R - 19 * (1 - $alpha)) / $alpha), 0, 255)
            $g = [int][Math]::Clamp([Math]::Round(($pixel.G - 18 * (1 - $alpha)) / $alpha), 0, 255)
            $b = [int][Math]::Clamp([Math]::Round(($pixel.B - 31 * (1 - $alpha)) / $alpha), 0, 255)
            $clean.SetPixel($x, $y, [Drawing.Color]::FromArgb($a, $r, $g, $b))
        }
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $clean.Save((Join-Path $Destination 'pawhide.png'), [Drawing.Imaging.ImageFormat]::Png)
    $sizes = @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
    $images = foreach ($size in $sizes) {
        $bitmap = [Drawing.Bitmap]::new($size, $size, [Drawing.Imaging.PixelFormat]::Format32bppPArgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
        $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.DrawImage($clean, [Drawing.Rectangle]::new(0, 0, $size, $size))
        $stream = [IO.MemoryStream]::new()
        $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
        [pscustomobject]@{ Size = $size; Bytes = $stream.ToArray() }
        $graphics.Dispose(); $bitmap.Dispose(); $stream.Dispose()
    }
    $file = [IO.File]::Create((Join-Path $Destination 'PawHide.ico'))
    $writer = [IO.BinaryWriter]::new($file)
    try {
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$images.Count)
        $offset = 6 + 16 * $images.Count
        foreach ($entry in $images) {
            $dimension = if ($entry.Size -eq 256) { 0 } else { $entry.Size }
            $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
            $writer.Write([byte]0); $writer.Write([byte]0); $writer.Write([uint16]1); $writer.Write([uint16]32)
            $writer.Write([uint32]$entry.Bytes.Length); $writer.Write([uint32]$offset)
            $offset += $entry.Bytes.Length
        }
        foreach ($entry in $images) { $writer.Write([byte[]]$entry.Bytes) }
    } finally { $writer.Dispose(); $file.Dispose() }
    if ($Comparison) {
        $sheet = [Drawing.Bitmap]::new(760, 330)
        $g = [Drawing.Graphics]::FromImage($sheet)
        try {
            $g.Clear([Drawing.Color]::FromArgb(28, 24, 36))
            $font = [Drawing.Font]::new('Segoe UI', 12)
            $brush = [Drawing.SolidBrush]::new([Drawing.Color]::White)
            $g.DrawString('Original', $font, $brush, 24, 13)
            $g.DrawString('Restored on charcoal', $font, $brush, 276, 13)
            $g.FillRectangle([Drawing.Brushes]::WhiteSmoke, 516, 0, 244, 330)
            $g.DrawString('Restored on light', $font, [Drawing.Brushes]::Black, 532, 13)
            $g.DrawImage($original, 24, 58, 220, 220)
            $g.DrawImage($clean, 276, 58, 220, 220)
            $g.DrawImage($clean, 528, 58, 220, 220)
            $g.DrawString('Original shape, artwork and inner colors preserved. Outer matte removed.', $font, $brush, 24, 300)
            $sheet.Save($Comparison, [Drawing.Imaging.ImageFormat]::Png)
            $font.Dispose(); $brush.Dispose()
        } finally { $g.Dispose(); $sheet.Dispose() }
    }
} finally { $original.Dispose(); $clean.Dispose() }
