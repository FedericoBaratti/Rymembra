# Il pacchetto MSIX per il Microsoft Store

Qui sta tutto quello che serve a trasformare Rymembra in un pacchetto MSIX da
caricare in Partner Center. L'installer NSIS (quello di GitHub) non cambia.

| File | A cosa serve |
|---|---|
| `AppxManifest.xml` | il modello del manifesto; i `{{SEGNAPOSTI}}` li riempie lo script |
| `identita.json` | i tre valori di Partner Center (non sono segreti) |
| `identita.esempio.json` | com'è fatto `identita.json` |
| `Assets/` | i loghi, nelle misure del Store |
| `genera-icone.ps1` | rigenera `Assets/` da `icons/icon.png` |
| `../../../../strumenti/msix.js` | costruisce la cartella e il `.msix` |

## Perché MSIX e non l'installer

Un'app inviata come EXE/MSI deve essere firmata con un certificato di una CA
fidata, e il Store non la firma al posto tuo. Un pacchetto MSIX invece lo firma
il Store dopo la certificazione: niente certificato a pagamento, niente avviso di
SmartScreen. Il costo è il pacchetto, che Tauri non sa costruire.

## La prima volta: l'identità

1. Partner Center → Apps e giochi → *Nuovo prodotto* → **MSIX o PWA app** →
   riserva il nome.
2. Gestione prodotto → **Identità del prodotto**.
3. Copia in `identita.json` (vedi `identita.esempio.json`):
   - `Package/Identity/Name` → `nome`
   - `Package/Identity/Publisher` → `editore` (comincia con `CN=`)
   - `Package/Properties/PublisherDisplayName` → `nomeEditore`

Devono coincidere **esattamente** con quelli del Store: un pacchetto con
un'identità diversa viene rifiutato al caricamento.

## Costruirlo

Dal tag, in CI: *Actions → msix → Run workflow*, con il tag (es. `v3.0.3`).
L'artefatto `rymembra-msix` contiene il `.msix`.

A mano, con il Windows SDK installato:

```powershell
cd apps/desktop
npx tauri build --no-bundle -- --locked
cd ../..
node strumenti/msix.js              # → target/msix/Rymembra_<versione>.0_x64.msix
node strumenti/msix.js --solo-cartella   # solo la cartella, per provarlo
```

`--no-bundle` è importante: l'eseguibile di un installer NSIS porta scritto
dentro il suo tipo di installer, e in un pacchetto MSIX quel dato sarebbe falso.

Il `.msix` **non è firmato**, ed è voluto.

## Provarlo prima di inviarlo

Il `.msix` non firmato non si installa su un computer vero. Si prova dalla
cartella, che Windows registra senza firma se è attiva la *Modalità sviluppatore*
(Impostazioni → Sistema → Per sviluppatori):

```powershell
Add-AppxPackage -Register target\msix\cartella\AppxManifest.xml
```

Poi si apre Rymembra dal menu Start e si controlla:

- Impostazioni → Aggiornamenti dice che a tenerla aggiornata è il Store, e
  l'interruttore e «Controlla adesso» ci sono ma non si possono usare;
  sotto «I documenti», la licenza, le licenze delle dipendenze, la privacy, le
  condizioni e la licenza dei caratteri si aprono anche senza rete, perché
  `msix.js` mette accanto all'eseguibile le stesse risorse dell'installer
  (`bundle.resources`);
- la libreria si apre, la musica suona, e dopo una chiusura e una riapertura la
  libreria e le impostazioni ci sono ancora;
- con una copia NSIS e la sua libreria sullo stesso PC, al primo avvio la
  proposta di copiare quella libreria compare una volta sola, la copia riesce
  e la cartella della copia NSIS resta com'era;
- Esplora, backup su Drive e scrobbling si collegano (i segreti vanno nel
  portachiavi di Windows, che il pacchetto non isola).

Si toglie con:

```powershell
Get-AppxPackage *Rymembra* | Remove-AppxPackage
```

Il Windows App Certification Kit (`appcert.exe`, nel Windows SDK) controlla le
stesse cose che controlla il Store: *Validate Store App*, scegliere Rymembra fra
le app installate.

## Inviarlo

Partner Center → l'app MSIX → *Start your submission* → **Packages** → carica il
`.msix`. La capacità `runFullTrust` va giustificata: Rymembra è un programma
Win32 che legge e scrive nelle cartelle di musica dell'utente, usa il
dispositivo audio e il portachiavi di Windows.

Ogni invio deve avere una versione **più alta** dell'ultimo, con la quarta cifra
a zero (`3.0.3.0`). La versione viene da `tauri.conf.json`, cioè da
`npm run version:set`.

## I dati dell'utente

Dentro un pacchetto MSIX, da Windows 10 1903 in poi, i file **nuovi** che il
programma crea in `%APPDATA%` e `%LOCALAPPDATA%` vanno in una cartella privata
sotto `%LOCALAPPDATA%\Packages\<nome del pacchetto>\`; quelli che esistono già
si usano dove sono. Per Rymembra è trasparente, con queste conseguenze:

- su un PC senza la copia NSIS, la libreria della copia del Store è sua e basta,
  e disinstallare il pacchetto da Windows cancella quella cartella;
- dalla 3.2.1 la copia del Store tiene la libreria in una cartella sua, col
  suffisso `.store`: `%APPDATA%\io.github.federicobaratti.rymembra.store`, e la
  cache in `%LOCALAPPDATA%\io.github.federicobaratti.rymembra.store`
  (`cartella_di_serie` in `lib.rs`, col marcatore `rymembra-store.txt`). Dal
  pacchetto quella cartella è nuova, quindi finisce tutta nella cartella
  privata. Prima la copia del Store apriva la libreria della copia NSIS dov'era,
  e il `-wal` e lo `-shm`, file nuovi, finivano nella cartella privata, lontano
  dal database;
- al primo avvio, una volta sola e solo se la sua libreria è ancora vuota
  (niente brani e niente playlist), la copia del Store propone di **copiare**
  quella che trova nella cartella senza suffisso: della copia NSIS, o sua di
  prima della 3.2.1, che dal pacchetto si vedono nello stesso posto
  (`trasloco::copia_la_libreria_di`). Il database si copia coi suoi file in una
  cartella di passaggio, si porta alla versione di questa build ed entra nella
  libreria; copertine e skin si copiano senza sovrascrivere niente; poi
  Rymembra si chiude. L'originale resta com'era. Se la proposta si rifiuta, non
  torna. La copia del Store non fa il trasloco dalla cartella di Aether;
- il Gestore credenziali di Windows non è virtualizzato: gli account collegati
  (servizio `io.github.federicobaratti.rymembra`) sono gli stessi per le due
  copie.
