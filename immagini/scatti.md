# Screenshot da rifare con musica libera

Le immagini di adesso mostrano una libreria vera, con copertine commerciali. Le nuove
devono mostrare solo musica che si può condividere, e una versione per lingua
dell'app.

## Preparare una libreria a parte

La libreria vera non si tocca. `RYMEMBRA_DATI` fa partire l'app su un'altra cartella
(`apps/desktop/src-tauri/src/main.rs`, `cartella_dati`):

```powershell
$env:RYMEMBRA_DATI = "C:\RymembraDemo"
& "<cartella d'installazione>\Rymembra.exe"
```

Da mettere dentro, 12–20 album **con copertina**:

- gli album di Scott Buckley (CC BY 4.0, scaricabili dal suo sito), coerenti con la
  colonna sonora del sito;
- album presi con **Esplora** dall'Internet Archive (netlabel, Live Music Archive)
  e da Audius: così le schermate mostrano anche la funzione vera;
- almeno **un FLAC 44,1 kHz / 24 bit** (la tappa Suono scrive
  «FLAC · 44.1 kHz · 24-bit · Stereo»).

Prima degli scatti fai suonare una decina di brani, così «Ascoltati di recente», la
coda e «Riprendi dov'eri» non sono vuoti. Poi annota quali album hai usato: vanno
citati nei crediti.

## Come scattare

- La finestra a **1440 × 900** (Studio a **1600 × 900**). Se Windows scala al
  125–150 % va bene: ridimensiono io.
- Cattura solo la finestra (Alt + Stamp, o lo Strumento di cattura), in **PNG**: la
  conversione in WebP la faccio io.
- Ogni scatto due volte: app in **italiano** e app in **inglese**. Il resto (album,
  brano, posizione) resta identico.

## Gli scatti

| File | Schermata | Cosa deve vedersi |
|---|---|---|
| `casa` | Home | Le raccolte della settimana piene, «Riprendi dov'eri» con un brano a metà, «Ascoltati di recente», la coda a destra con 6–8 brani |
| `fonti` | Impostazioni → «Da dove viene la musica» | La scheda intera, con le fonti e le loro regole |
| `brani` | Brani | L'elenco con un brano in riproduzione (va solo nel README) |
| `riproduzione` | In riproduzione, scena dello spettro | Il FLAC in riproduzione, con la riga del formato leggibile |
| `album` | Album, skin **Plain** | Almeno 12 copertine |
| `skin` | Album, skin **Neon** | **La stessa** finestra, la stessa dimensione e lo stesso scorrimento di `album`: cambia solo la skin. Sul sito una tendina passa dall'una all'altra, e se gli album si spostano si vede |
| `studio` | Studio sulla pagina Album | I livelli a sinistra, la pagina al centro, l'ispettore a destra |

## Dove vanno

Mandami i PNG con nomi come `casa-it.png` e `casa-en.png`; li sistemo io.

- Gli scatti in inglese diventano `immagini/<nome>.webp`, usati dal README e dalle
  pagine EN, FR, DE, ES, PT e PL.
- Gli scatti in italiano diventano `immagini/it/<nome>.webp`, usati da `/it/`.
