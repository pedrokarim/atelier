# Génère l'icône d'Atelier : une grille de quatre tuiles sur un carré arrondi bleu.
#
# Produit assets/atelier.ico (plusieurs tailles, entrées PNG) et
# assets/atelier-64.png (zone de notification).
#
# Usage : powershell.exe -NoProfile -File scripts\make-icon.ps1

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$assets = Join-Path (Split-Path $PSScriptRoot -Parent) 'assets'
New-Item -ItemType Directory -Force $assets | Out-Null

function New-IconBitmap([int]$size) {
    $bitmap = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    # Fond : carré arrondi en dégradé diagonal.
    $s = [single]$size
    $radius = $s * 0.24
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $radius * 2
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc($s - $d, 0, $d, $d, 270, 90)
    $path.AddArc($s - $d, $s - $d, $d, $d, 0, 90)
    $path.AddArc(0, $s - $d, $d, $d, 90, 90)
    $path.CloseFigure()

    $top = [System.Drawing.Color]::FromArgb(255, 0x55, 0x98, 0xE7)
    $bottom = [System.Drawing.Color]::FromArgb(255, 0x1C, 0x5C, 0xAB)
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF(0, 0)),
        (New-Object System.Drawing.PointF($s, $s)),
        $top, $bottom)
    $g.FillPath($brush, $path)

    # Quatre tuiles blanches : la grille de widgets. La tuile en haut à droite
    # est translucide, pour que la grille se lise même à 16 px.
    $margin = $s * 0.22
    $gap = [Math]::Max(1.0, $s * 0.07)
    $tile = ($s - 2 * $margin - $gap) / 2
    $corner = [Math]::Max(1.0, $tile * 0.28)
    $solid = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::White)
    $soft = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(150, 255, 255, 255))
    foreach ($cell in @(@(0, 0, $solid), @(1, 0, $soft), @(0, 1, $solid), @(1, 1, $solid))) {
        $x = $margin + $cell[0] * ($tile + $gap)
        $y = $margin + $cell[1] * ($tile + $gap)
        $shape = New-Object System.Drawing.Drawing2D.GraphicsPath
        $c = $corner * 2
        $shape.AddArc($x, $y, $c, $c, 180, 90)
        $shape.AddArc($x + $tile - $c, $y, $c, $c, 270, 90)
        $shape.AddArc($x + $tile - $c, $y + $tile - $c, $c, $c, 0, 90)
        $shape.AddArc($x, $y + $tile - $c, $c, $c, 90, 90)
        $shape.CloseFigure()
        $g.FillPath($cell[2], $shape)
        $shape.Dispose()
    }
    $solid.Dispose()
    $soft.Dispose()
    $brush.Dispose()
    $path.Dispose()
    $g.Dispose()
    return $bitmap
}

function Get-PngBytes($bitmap) {
    $stream = New-Object System.IO.MemoryStream
    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    return , $stream.ToArray()
}

$sizes = 16, 20, 24, 32, 40, 48, 64, 128, 256
$images = foreach ($size in $sizes) {
    $bitmap = New-IconBitmap $size
    $bytes = Get-PngBytes $bitmap
    if ($size -eq 64) { [System.IO.File]::WriteAllBytes((Join-Path $assets 'atelier-64.png'), $bytes) }
    $bitmap.Dispose()
    [pscustomobject]@{ Size = $size; Bytes = $bytes }
}

# Format ICO : en-tête, une entrée de 16 octets par taille, puis les PNG.
$out = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter($out)
$writer.Write([UInt16]0)
$writer.Write([UInt16]1)
$writer.Write([UInt16]$images.Count)

$offset = 6 + 16 * $images.Count
foreach ($image in $images) {
    $dimension = if ($image.Size -ge 256) { 0 } else { $image.Size }
    $writer.Write([byte]$dimension)
    $writer.Write([byte]$dimension)
    $writer.Write([byte]0)
    $writer.Write([byte]0)
    $writer.Write([UInt16]1)
    $writer.Write([UInt16]32)
    $writer.Write([UInt32]$image.Bytes.Length)
    $writer.Write([UInt32]$offset)
    $offset += $image.Bytes.Length
}
foreach ($image in $images) { $writer.Write($image.Bytes) }
$writer.Flush()

[System.IO.File]::WriteAllBytes((Join-Path $assets 'atelier.ico'), $out.ToArray())
Write-Host "Icône écrite dans $assets"
