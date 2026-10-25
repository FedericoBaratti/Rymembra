# I loghi del pacchetto MSIX, ricavati da `icons/icon.png`.
#
#   powershell -NoProfile -File apps/desktop/src-tauri/msix/genera-icone.ps1
#
# Non gira in CI: i PNG stanno nel repo, e questo serve solo quando l'icona
# cambia. Scrive in `Assets/` accanto a questo file.
#
# Le misure sono quelle del Store: tre loghi — `Square44x44Logo` (elenco app,
# barra delle applicazioni), `Square150x150Logo` (tessera di Start),
# `StoreLogo` (la voce nel Microsoft Store e nelle Impostazioni) — ciascuno
# alle scale 100, 200 e 400 per cento. Per il 44 ci sono anche le versioni
# `targetsize-*`, con e senza `altform-unplated`: Windows le sceglie per la
# barra delle applicazioni e per l'elenco di Start, e senza la variante
# «unplated» mette l'icona su un quadrato colorato che con questa icona, già
# scura, si vedrebbe come una cornice.
#
# L'origine è 512×512: alla scala 400 la tessera da 150 chiede 600 pixel, cioè
# un piccolo ingrandimento. L'icona è morbida di suo (ha un alone), quindi non
# si nota; se un giorno ci fosse l'originale vettoriale, partire da quello.

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$qui = Split-Path -Parent $MyInvocation.MyCommand.Path
$origine = Join-Path $qui '..\icons\icon.png' | Resolve-Path
$uscita = Join-Path $qui 'Assets'
New-Item -ItemType Directory -Force $uscita | Out-Null

$sorgente = [System.Drawing.Image]::FromFile($origine)

function Scrivi([string]$nome, [int]$lato) {
    $bmp = New-Object System.Drawing.Bitmap $lato, $lato, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.DrawImage($sorgente, 0, 0, $lato, $lato)
    $g.Dispose()
    $bmp.Save((Join-Path $uscita $nome), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    '{0,-58} {1}x{1}' -f $nome, $lato
}

# Tre loghi, tre scale.
foreach ($scala in 100, 200, 400) {
    $f = $scala / 100
    Scrivi "Square44x44Logo.scale-$scala.png" ([int][math]::Round(44 * $f))
    Scrivi "Square150x150Logo.scale-$scala.png" ([int][math]::Round(150 * $f))
    Scrivi "StoreLogo.scale-$scala.png" ([int][math]::Round(50 * $f))
}

# L'icona di Start e della barra delle applicazioni.
foreach ($lato in 16, 24, 32, 48, 256) {
    Scrivi "Square44x44Logo.targetsize-$lato.png" $lato
    Scrivi "Square44x44Logo.targetsize-${lato}_altform-unplated.png" $lato
}

$sorgente.Dispose()
