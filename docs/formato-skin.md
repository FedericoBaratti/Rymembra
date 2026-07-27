# Il formato `.aeskin`

Riferimento del formato skin di Aether. Descrive cosa una skin può dichiarare, cosa
non può, e perché.

## La regola che spiega tutto il resto

**Una skin è dati, non CSS.** Il compilatore è l'unico autore di CSS del sistema, e
nessun valore scritto da un autore viene mai copiato nell'output: un colore entra
come canali, una lunghezza come numero più unità, una curva come quattro numeri.

La conseguenza pratica: l'intera classe di attacchi via CSS — `url()` che chiama a
casa, `@import` che carica un foglio remoto, selettori che esfiltrano il contenuto
degli attributi — non si applica, perché non esiste un canale in cui infilarli. Ma
la garanzia vale **solo** se ogni campo è tipizzato: basta uno che accetti una
stringa e la copi, e salta tutta insieme. Per questo la validazione rifiuta anche
cose innocue come `calc()` e i nomi di colore CSS.

## La struttura

```jsonc
{
  "format": 1,
  "id": "nocturne",                    // minuscole, cifre, trattini
  "meta": { "name", "author", "version", "description?", "license?", "basedOn?" },
  "capabilities": { "light", "mobile", "dynamicAccent" },
  "palette":  { "teal": "#00f0ff" },   // colori locali, nominati
  "tokens":   { /* il contratto con i componenti */ },
  "themes":   { "light": { /* solo i token che cambiano */ } },
  "platforms":{ "mobile": { /* idem */ } },
  "motion":   { "intensity", "easings?", "routeTransition?" },
  "layout":   { "player", "sidebar", "density" },
  "patterns": { "grid": { "effect": "hairlineGrid", … } },
  "parts":    { "section-card": { /* superfici ridisegnate */ } }
}
```

Ogni blocco è `strict()`: una chiave sconosciuta è un **errore**, mai silenzio. È la
scelta opposta a quella comoda, e il motivo è che nel sistema precedente una skin
era CSS — un token scritto male non dava errore, dava una skin visivamente rotta in
un punto solo, che si poteva notare settimane dopo.

## I valori

| tipo | forma | esempio |
|---|---|---|
| colore | esadecimale o `rgb()`/`rgba()` | `"#8b7cf6"`, `"rgba(255,255,255,0.6)"` |
| riferimento a token | `{ "$token": id }` | `{ "$token": "color.accent" }` |
| riferimento a tavolozza | `{ "$palette": nome, "alpha"? }` | `{ "$palette": "teal", "alpha": 0.18 }` |
| legato alla copertina | `{ "$source": …, "alpha"? }` | `{ "$source": "albumArt.vibrant" }` |
| lunghezza | numero + unità da lista chiusa | `"14px"`, `"3cqw"` |
| lunghezza adattiva | `{ min, preferred, max }` | diventa `clamp()` |
| durata | `ms` o `s`, massimo 10s | `"280ms"` |
| easing | curva, passi, o parola chiave | `{ "kind": "cubicBezier", "points": [0.16,1,0.3,1] }` |
| ombra | lista di livelli | `{ "layers": [ { x, y, blur, spread?, color, inset? } ] }` |
| carattere | solo nomi di famiglia | `["Space Grotesk Variable"]` |

**Rifiutati**, e verificati da test: `calc()`, `clamp()` come stringa, `currentColor`,
i nomi di colore CSS, `color-mix()`, `url()`, le unità fuori lista, e qualunque cosa
contenga virgolette o punti e virgola.

Un'ombra con `layers: []` diventa `none` — è così che una skin piatta spegne
l'elevazione senza inventare un'ombra trasparente.

## I token derivati e calcolati

Non si dichiarano: li emette il compilatore.

- **`--accent-rgb`** e le altre triple, derivate dal colore. Erano due dichiarazioni
  da tenere allineate a mano.
- **`--surface-0-rgb`**, derivata da `color.surface.0`. Sostituisce il commento in
  maiuscolo che diceva «DEVE combaciare con surface-0» accanto alla nebbia di
  cyberpunk.
- **`--shell-left`**, **`--player-clearance`**, **`--transition-fast`**,
  **`--transition-med`**: sono conseguenze, non scelte. Una skin non deve poterle
  contraddire.
- **`color-scheme`**: `dark` nel blocco base, `light` nel tema chiaro. Dimenticarlo
  dà barre di scorrimento chiare su fondo nero.

## Gli effetti, e il loro costo

Undici effetti parametrici, ognuno con una classe di costo dichiarata:

| classe | peso | effetti |
|---|---|---|
| `cheap` | 1 | `solid`, `linearGradient`, `radialGradient`, `vignette` |
| `paint` | 3 | `conicGradient`, `hairlineGrid`, `scanlines`, `stripes`, `dotGrid` |
| `composited` | 4 | `chamfer` |
| `gpu` | 10 | `blurBehind` |

Il budget di una superficie è **10**. Non è documentazione: `global.css` portava la
nota scritta a mano «al massimo ~4 superfici con `backdrop-filter` composte
insieme», e ora quel limite è un numero che si può controllare — due `blurBehind`
sforano.

## Le parti

Il registro elenca 46 superfici ridisegnabili, con nome, gruppo e descrizione. Le
classi esistevano già nei componenti ma **non erano documentate da nessuna parte**:
per sapere quali agganci esistevano si doveva leggere il CSS delle skin già fatte.

Cosa una skin può cambiare su una parte: `background` (livelli di effetto),
`textColor`, `borderColor`, `borderWidth`, `radius`, `clip`, `opacity`,
`letterSpacing`, `textTransform`, `fontWeight`, un `layer` su `::after`, e quattro
`states`.

Cosa **non** può, per scelta: `width`, `height`, `margin`, `padding`, `position`,
`display`. Una skin che può spostare le cose può anche sovrapporle o portarle fuori
schermo, e il risultato non sarebbe una skin brutta ma un'app inutilizzabile che
sembra un bug dell'app.

Due garanzie strutturali negli stati: il selettore usa `:where()`, così la
specificità resta quella della parte e uno stato dichiarato da una skin non vince
su una regola che il componente considera più importante; e `focus` è
`:focus-visible`, così lo stato appare a chi naviga da tastiera e non a ogni clic.

## Il pacchetto

```
Nocturne.aeskin
├─ skin.json      il manifest, unica voce obbligatoria
├─ preview.png    miniatura per il selettore
└─ assets/        png, jpg, webp, woff2 — un solo livello
```

Un pacchetto arriva da fuori, quindi si assume che chi l'ha costruito possa averlo
fatto in malafede. Tre difese:

- **path traversal**: lista chiusa di nomi ammessi, quindi non esiste un percorso da
  normalizzare;
- **zip bomb**: dimensione per voce, numero di voci e rapporto di compressione,
  controllati **prima** di decomprimere leggendo la struttura dell'archivio;
- **tipo mentito**: il tipo dai byte iniziali, non dall'estensione. L'SVG è escluso:
  è un documento e può contenere script.

Il pacchetto porta due forme dello stesso manifest: `document` validato per il
compilatore, e `source` come l'autore l'ha scritta. Servono entrambe perché **la
forma interna non è serializzabile come sorgente** — un colore validato è un
oggetto di canali, e riscriverlo produrrebbe un manifest che la validazione
rifiuta.

## Come si aggiunge un token

1. Si aggiunge la voce a `TOKENS` in `packages/skin/src/tokens.ts`, col suo nome CSS,
   il tipo, il gruppo e una descrizione utile a chi la leggerà nello Studio.
2. Non serve altro: la validazione, il pannello dello Studio e il compilatore si
   derivano dal registro.
3. Se il token è obbligatorio, `plain` deve dichiararlo — è la skin di riferimento, e
   il suo test di fedeltà lo verifica.

## Come si aggiunge un effetto

1. Si aggiunge il ramo a `effectSchema` in `effects.ts`.
2. Si dichiara il costo in `EFFECT_COST` e il bersaglio in `EFFECT_TARGET`. Sono
   mappe esaustive: un effetto senza costo non compila.
3. Si aggiunge il ramo a `compileEffect` in `compile.ts`. Lo `switch` è esaustivo per
   lo stesso motivo.
