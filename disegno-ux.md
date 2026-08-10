# Il disegno di Aether

Questo documento è la consegna **A** di `prompt-redesign-ux-ui.md`: cosa
l'interfaccia decide, e perché. Le schermate stanno nella tela di Claude Design
(`Aether - impalcatura.dc.html`), il codice in `apps/desktop/src`.

I numeri qui dentro non sono ricordati: i contrasti li stampa
`cargo run -p aether-skin --example contrasti`, i token li dichiara
`core/aether-skin/src/tokens.rs`, le parti `parts.rs`. Se una tabella e il
codice divergono, ha ragione il codice — e questa riga è un invito a
correggere la tabella.

---

## 1. I cinque principi

### 1. La copertina è l'unica cosa colorata, ed è anche la luce

Quattro superfici neutre, un accento, e una sola cosa che prende il colore dalla
musica: l'**ambiente**, dietro la copertina nella terza colonna e dietro la
schermata In riproduzione. **Mai il testo, mai una superficie che deve restare
leggibile.**

La regola non è estetica, è di contrasto: la tinta di una copertina è
imprevedibile, e un titolo su un colore imprevedibile è un titolo che a volte
non si legge. In `np-screen` ci sono due strati apposta — l'ambiente colorato, e
sopra `np-scrim`, che è un velo **neutro** scritto con `--surface-0-rgb`. Se il
velo fosse una versione scura della tinta, con una copertina gialla il titolo si
leggerebbe su giallo scuro, che è ancora giallo.

L'ambiente non è una tinta estratta: è **la copertina stessa**, ingrandita e
sfocata di sessantaquattro pixel (`Sfocata` in `Copertina.tsx`, la classe
`.tinta`). È meglio di un colore medio perché una copertina non ha *un* colore:
un disco rosso con la fascia gialla dà un ambiente rosso e giallo, ognuno dalla
parte in cui sta. Sotto resta la sfumatura `--hero-rgb`, che è quel che si vede
quando la copertina non c'è — cioè su metà di una libreria vera.

Sempre la **miniatura**, anche quando la copertina accanto è piena: sotto una
sfocatura così i dettagli di un originale da mille pixel sono byte chiesti al
disco per essere buttati. La copertina piena la chiedono i tre posti in cui è
lei il soggetto — schermo intero, terza colonna, testata di un album — e nessun
altro: una griglia di novecento originali sarebbero novecento richieste per
disegnare dei quadratini da centonovanta.

### 2. Dove vado sta a sinistra, cosa configuro sta in una pagina

La barra laterale conteneva cinque blocchi, di cui uno solo era navigazione.
«Scansiona» — un comando che legge diecimila file e dura venti secondi — stava
alla stessa distanza dal dito di «Album».

Ora a sinistra ci sono quattro destinazioni più Impostazioni, appuntata in
fondo. Tutto il resto è una pagina con sei sezioni e la larghezza per spiegare
cosa fa: un comando che sposta dei file merita una riga di testo accanto, e in
una colonna da 240 pixel quella riga non ci stava — quindi non c'era.

### 3. Denso nelle righe, largo nelle intestazioni

Righe da **40 px**. È la misura in cui una miniatura da 34 sta comoda e due
righe di testo — titolo 13.5, artista 12 — non si toccano. L'aria si spende
tutta sopra l'elenco (24 px sopra il titolo, 14 sotto) e **nessuna** fra una riga
e l'altra: un elenco fitto si scorre, un elenco arioso si sfoglia, e una libreria
musicale si scorre.

### 4. Ogni comando ha almeno due porte

Il tasto destro non è mai l'unica. «Riproduci dopo», «Accoda» e «Aggiungi a
playlist» esistevano solo nel menù contestuale: invisibili a chi non prova il
tasto destro, irraggiungibili per chi non ha un mouse. Ora sono anche nella
barra della selezione multipla.

`apriMenu(e, elenco: number[])` prendeva già un array e veniva chiamato sempre
con un elemento solo. La selezione multipla è quel parametro che finalmente
serve a qualcosa.

### 5. Il markup nomina le parti

`className="semantica parte"`, in quest'ordine, **sempre additivo**:

```tsx
<div className="riga list-row">          // sì
<div className="list-row">               // no: si perde la semantica locale
<div className="list-row riga">          // no: l'ordine è un contratto di lettura
```

Costa una classe. `compila_parte` in `core/aether-skin/src/compile.rs` emette
`:root[data-skin='<id>'] .<nome>`, quindi una parte **è** una classe: emetterla
non richiede nulla di più che scriverla.

È anche ciò che rende possibile la sonda dello Studio, che cammina nel DOM e
confronta le classi col registro. Non servono attributi in più perché il
contratto è già scritto nel markup.

---

## 2. La mappa dei token

47 token, dichiarati in `core/aether-skin/src/tokens.rs`. **21 obbligatori.**
Qui i gruppi e a cosa servono; i nomi CSS esatti li emette il compilatore.

| Gruppo | Token | Nome CSS | Nota |
|---|---|---|---|
| Tipografia | `font.sans` | `--font-sans` | Inter Variable, ora **impacchettato** |
| | `font.mono` | `--font-mono` | durate, conteggi, percorsi, indici |
| | `font.display` | `--font-dot` | il carattere dei titoli. Il nome è storico — è nato nella skin Nothing |
| Superfici | `color.surface.0..3` | `--color-surface-0..3` | fondo, pannello, scheda, sopraelevazione |
| Testo | `color.text.1..3` | `--color-text-1..3` | primario, secondario, terziario |
| Accento | `color.accent` + `.soft` `.glow` `.like` | `--accent*`, `--accent-like` | `.like` esiste perché una skin possa portare il cuore al rosso senza toccare l'accento di tutto il resto |
| | `color.hero` | `--hero-rgb` | l'unico che segue la copertina |
| Stati | `color.danger/success/warning` + `.soft` | `--danger*` ecc. | |
| Cornice | `color.sidebar`, `color.hairline`, `color.ambient.1/2` | `--sidebar-bg`, `--hairline`, `--ambient-1/2` | |
| Impaginazione | `layout.rail`, `.railExpanded`, `.playerHeight`, `.playerGap`, `.contentX` | `--rail-w` ecc. | |
| Geometria | `radius.panel`, `radius.card` | `--radius-panel`, `--radius-card` | |
| Sopraelevazione | `shadow.1..3`, `shadow.player`, `glow.accent` | `--shadow-*`, `--glow-accent` | |
| Movimento | `motion.ease.*`, `motion.dur.1..3` | `--ease-*`, `--dur-*` | |
| Tela | `canvas.viz.*`, `canvas.scrubber.*` | `--viz-*`, `--scrubber-*` | letti da canvas che oggi non esistono |

### La spaziatura non è un token, e non lo diventa

`--spazio-1..5` (4/8/14/22/34 px) e `--colonna-w` stanno in `stile.css`. Il
registro dei token è il contratto dei **colori**, della geometria e del
movimento. Una skin che potesse cambiare la spaziatura potrebbe far uscire le
cose dallo schermo — ed è il confine che il formato traccia apposta, lo stesso
per cui il vocabolario di una parte non contiene `display` né `position`.

### Nemmeno la luce, e per la stessa ragione

Accanto a `--spazio-*` stanno cinque proprietà che descrivono **da dove viene la
luce**, e nessuna è un token:

| Nome | Cos'è |
|---|---|
| `--spigolo` | il filo chiaro sul bordo alto di una superficie sollevata |
| `--incavo` | il fondo scuro di un solco: una pista, un campo, una barra |
| `--alzata` | quanto schiarisce la cima di una superficie |
| `--luce` | `--alzata` già composta in una sfumatura verticale |
| `--filo` | `--spigolo` già composto in un'ombra interna da un pixel |

Più i due raggi derivati, `--raggio-interno` (0.6 × `--radius-card`) e
`--raggio-minuto` (0.36 ×). Erano sei numeri fissi — 4, 5, 6, 8, 10 — dentro
contenitori che usavano `--radius-card`: una skin che squadrava tutto lasciava
sei angoli tondi, e con la moltiplicazione zero resta zero.

Non sono colori: sono il **modo** in cui i colori della skin si compongono per
dire che una superficie è sollevata o affondata. Si scrivono con `color-mix()`
dentro `light-dark()`, e mescolano sempre verso il token più chiaro che c'è — il
testo al buio, il fondo alla luce — così la direzione si inverte da sé fra i due
temi senza che nessuno la dichiari due volte. `light-dark()` legge
`color-scheme`, che il compilatore emette già (`dark` sul blocco base, `light`
sulla variante chiara).

Una skin che vuole altro non le tocca: ridipinge la parte, e vince per
specificità — `:root[data-skin='…'] .parte` è 0-3-0 contro lo 0-1-0 delle classi
semantiche.

### Le ombre hanno due strati

`shadow.1..3` e `shadow.player` avevano un livello solo, ereditato da
`global.css`. Un livello non può dire due cose insieme: dove un oggetto **tocca**
(scostamento corto, sfocatura corta, quasi opaca) e quanto sta **in alto**
(scostamento lungo, sfocatura larga, trasparente). Ora ognuna ne ha almeno due, e
la proprietà è provata in `core/aether-skin/tests/fedelta.rs`
(`le_ombre_hanno_un_contatto_e_un_ambiente`) invece di essere un valore fissato
che nessuno può misurare.

### Due valori che sono cambiati, e perché

- **`color.text.3`: `.38` → `.46`.** `rgba(255,255,255,.38)` su `#09090d` fa
  **3.47:1**, sotto la soglia WCAG. Era un numero scritto a occhio in un file
  che nessuno poteva misurare — e sull'albero mobile qualcuno l'aveva già alzato
  a mano dopo averlo visto su un telefono, cioè lo stesso difetto scoperto due
  volte sul dispositivo invece che una volta nel codice.
- **La variante chiara esisteva a metà.** `capabilities.light` diceva `true` e
  `themes.light` sovrascriveva otto token su quaranta: superfici e testo
  restavano scuri. L'interruttore del tema sarebbe apparso e non avrebbe fatto
  quasi niente — che è esattamente l'avviso `UnkeptCapability`, solo troppo
  debole per accorgersene.

---

## 3. La mappa delle parti

**51 parti**, di cui **45 emesse dal markup**. Le sei che non lo sono, con la
ragione:

| Parte | Perché no |
|---|---|
| `lyrics-screen`, `lyric-line` | Il nucleo non espone testi. La colonna dei testi esiste in `tracks.lyrics`, ma nessun comando la legge |
| `eq-slider` | I cursori dell'equalizzatore usano `range-accent`, che è la parte del cursore a scorrimento e vale già per tutti. Una parte per il singolo cursore di banda darebbe alle skin un secondo modo di dire la stessa cosa |
| `tour-tooltip` | Non c'è una presentazione guidata, e inventarne una per riempire una parte è il ragionamento al contrario |
| `bottom-nav` | Il minimo della finestra è 880 px e la barra laterale ci sta. Resta nel registro per Android |
| `home-shortcuts` | Non c'è una schermata iniziale. Aether apre sulla libreria, che è quel che si è venuti a fare |

`skeleton` era emessa **solo** dall'anteprima dello Studio: l'applicazione
mostrava una parte che non usava, perché `caricaVista` aspettava e poi
sostituiva, lasciando in piedi il contenuto della vista precedente. Ora la
libreria ha i suoi segnaposto — dodici schede o dieci righe, della stessa misura
di quelle vere, così l'arrivo dei dati non fa saltare la pagina.

Le tre chieste al registro in questo ridisegno, e concesse:

- **`list-row`** — senza, una skin può ridisegnare la griglia ma non lo stato
  attivo di una riga: l'unica superficie che si guarda per ore.
- **`segmented`** — altrimenti le linguette abusano di `nav-pill`, che è
  un'altra cosa. Un `nav-pill` porta **altrove**; una linguetta cambia il
  **taglio** dello stesso contenuto.
- **`selection-bar`** — la barra che prende il posto del lettore quando ci sono
  più brani scelti.

E un campo di documento: **`meta.preview`**, tre colori scelti dall'autore per
la scheda della skin. Prima venivano indovinati prendendo fondo, superficie e
accento — che per una skin sobria dà tre grigi quasi uguali.

---

## 4. Le scale

### Tipografia

| Uso | Misura | Peso | Tracking | Carattere |
|---|---|---|---|---|
| `np-title` (schermo intero) | `clamp(20, 8cqw, 40)` / 1.08 | 700 | −0.032em | **display** |
| `page-title` | 26 / 1.1 | 600 | −0.02em | **display** |
| Titolo colonna «in riproduzione» | 19 / 1.25 | 600 | −0.015em | **display** |
| `stat-number` | 22 | 600 | −0.02em | **display** |
| Marchio «Aether» | 16 | 600 | −0.015em | **display** |
| Titolo di stato vuoto | 20 | 400 | −0.015em | **display** |
| Corpo, riga di elenco | 13.5 | 400 | — | sans |
| Nav, sotto-titolo di riga | 12–13 | 400 | — | sans |
| `page-subtitle` | 12.5 | 400 | — | sans |
| Occhielli, intestazioni di colonna | 10–10.5 | 600 | +0.13…0.16em, maiuscolo | **mono** |
| Durate, conteggi, percorsi | 11–12 | 400 | — | **mono** |

Il mono non è decorazione: sono numeri che si confrontano in colonna. `0:59` e
`1:00` devono avere la stessa larghezza, altrimenti il cursore salta di qualche
pixel una volta al minuto. Dove un numero è in sans e cambia sul posto — il
conteggio della selezione multipla, i numeri delle statistiche mentre una
scansione va avanti — c'è `font-variant-numeric: tabular-nums`, che compra la
stessa proprietà senza cambiare carattere.

**Il carattere da display sta in sei posti e non oltre.** Sono i titoli che
*sono* la pagina più il marchio; se arrivasse alle etichette smetterebbe di
essere un accento. `--font-dot` si chiama così per ragioni storiche — è nato
nella skin Nothing — ed è il posto in cui una skin mette la sua faccia: `Sala`
ci mette il monospaziato, e le sue pagine si intitolano come apparecchiature.

**Il titolo dello schermo intero è fluido e sta su due righe.** Era 38 punti
fissi su una riga sola, e con la coda aperta il centro è largo poco più di
trecento pixel: «Solar Sailer - Remixed by Pretty Lights» si leggeva «Solar
Sailer - …», cioè senza la parte che distingue una versione dall'altra. Il `cqw`
si misura su `.centro`, che è un contenitore apposta: il `cq` più vicino sarebbe
stato la zona del contenuto, larga il doppio.

**Dove il testo va a capo da solo** — titoli di stati vuoti, note, spiegazioni
delle opzioni — c'è `text-wrap: balance` o `pretty`. Tolgono la riga orfana di
una parola sola, che è il difetto che fa sembrare sciatto un testo altrimenti
giusto.

### Spaziatura

`4 · 8 · 14 · 22 · 34`. Non una scala geometrica pura: i passi bassi servono a
comporre controlli (padding di un bottone, distanza fra un'icona e la sua
etichetta) e vogliono essere fitti; quelli alti separano blocchi e vogliono
respirare.

### Larghezze

| Da | A | Cosa succede |
|---|---|---|
| 880 (minimo della finestra) | 1100 | Due colonne. Il lettore torna flottante, la coda è un pannello |
| 1100 | 1280 | Tre colonne. La griglia degli album scende a 3–4 schede per riga |
| 1280 (misura di disegno) | 1600 | Tre colonne comode, 5 schede per riga |
| 1600 | ∞ | Il contenuto continua a distribuirsi; `--content-x` è `clamp(16px, 3cqw, 48px)` |

La terza colonna **si chiude da sé** sotto 1100 px e **non si riapre da sé**
quando la finestra torna larga: riaprirla annullerebbe una chiusura decisa a
mano.

---

## 5. Le icone

38 simboli, `viewBox="0 0 20 20"`, `stroke-width` fra 1.5 e 2.6, tutti
`currentColor`. Montati una volta in `parti/Icone.tsx` come sprite `<symbol>`,
usati con `<use href="#i-play">`.

Nessuna libreria: costano meno di sei kilobyte e non portano un albero di
pacchetti da aggiornare. La CSP è chiusa (`default-src 'self'`), quindi un font
di icone remoto non sarebbe nemmeno caricabile.

**Un solo colore scritto in tutto lo sprite**: il secondo arco di `i-mark`, ed è
`var(--skin-color-ciano)` — il token della tavolozza, non un letterale.

Cosa hanno sostituito: `⇄ ⏮ ⏸ ⏭ ↻ ♥ ♡ ★ ☆ ☰ ✕ ▶ ⚙`, più le due emoji a colori
`🔇 🔊` — che si disegnavano con la tavolozza del carattere di sistema, cioè
erano gli unici due colori dell'interfaccia che nessuna skin poteva toccare.

---

## 6. Il movimento

Tre durate (`150 / 280 / 450 ms`) e due curve (`--ease-out-expo`,
`--ease-spring`). Si animano **solo** opacità e trasformazione: sono le due
proprietà che il compositore anima senza ridisegnare, e il vincolo è del
compilatore — `RouteFrame` accetta solo `opacity`, `scale` e `translateY`.

`motion.intensity` della skin arriva alla finestra come `data-motion` sulla
radice. **`prefers-reduced-motion` del sistema vince sempre**, e non è
configurabile: una preferenza di accessibilità che una skin può sovrascrivere
non è una preferenza.

### Il cambio di vista, che la skin scriveva già

`motion.routeTransition` è dichiarato in `plain.json` dal primo giorno, e il
compilatore ne emette da sempre le regole complete — due `@keyframes` più
`::view-transition-old(root)` e `::view-transition-new(root)`. Mancava una riga:
nessuno chiamava `document.startViewTransition`, quindi il motore non produceva
mai le pseudo-elemento su cui quelle regole agiscono. Ora lo chiama
`cambiandoVista` (`apps/desktop/src/transizione.ts`), e il foglio
dell'applicazione non contiene **nessuna** durata né curva per quel passaggio:
sono della skin.

Chi non partecipa porta un `view-transition-name` proprio — navigazione,
lettore, terza colonna, coda, barra della selezione, notifica. Non vuol dire che
non si muovano: vuol dire che non ereditano la scala e lo scorrimento scritti
per la *pagina*. Per il lettore c'è una seconda ragione, tecnica: è `position:
fixed`, e un elemento fisso dentro l'istantanea della radice viene fotografato
dove starebbe se scorresse.

Due porte chiudono la transizione: `prefers-reduced-motion`, e `--motion-scale`
letta a mano — le regole del compilatore usano `var(--dur-2)` nudo, senza la
scala, quindi una skin che si dichiara `none` avrebbe animato lo stesso.

### Cosa si muove, e perché ognuna è una risposta

| Cosa | Come | Perché |
|---|---|---|
| Cambio di vista | quel che dice la skin | vedi sopra |
| Menù, finestrelle, notifiche, suggerimenti, pannello eq | `@starting-style`, opacità + `scale(.96)`, `--dur-2` | un menù che sboccia dal puntatore dice da dove viene; l'origine è il punto in cui è avvenuto il gesto |
| Scheda di un album, al sorvolo | sale di 3 px, l'ombra passa da `--shadow-2` a `--shadow-3` | la risposta di un oggetto. Prima era un contorno d'accento, cioè il segno del **fuoco** usato per il puntatore |
| Tasto grande di riproduzione | `scale(1.05)` al sorvolo, `.93` alla pressione, `--ease-spring` | è l'unico posto in cui una molla ha senso: il tasto torna su |
| Cuore | un battito solo quando `aria-pressed` diventa vero | l'aggiornamento è ottimistico, e il battito rende quel colore un evento invece di un fatto |
| Copertine | dissolvenza all'arrivo, `--dur-2` | `loading="lazy"` le fa arrivare mentre si scorre: quaranta comparse istantanee sono uno sfarfallio |
| Segnaposto di caricamento | un riflesso che attraversa, 1.6 s | vedi §11 |

**Non c'è una comparsa scaglionata di righe e schede.** Ci stava, ed è stata
tolta: il cambio di vista ha già il suo movimento, e sommarne un secondo su ogni
elemento aggiunge trecento millisecondi di attesa percepita a un elenco che
esiste per essere scorso.

**E non ci sono barre animate sulla riga che suona.** Uno spettro finto è l'unica
bugia che questa interfaccia si è vietata, e tre barrette che si muovono su una
riga la raccontano lo stesso.

### Lo spettro, che invece è vero

`viz-screen`, `viz-title` ed `eq-bars` erano parti registrate che nessuno
emetteva, e i token `--viz-*` erano dichiarati «letti da canvas che oggi non
esistono». Adesso la canvas c'è, in `parti/Spettro.tsx`, dentro `np-screen`.

La regola non è cambiata — quel che si disegna viene dal suono che esce — è
cambiato che adesso il suono si può guardare. `aether-play::spettro` prende i
campioni **nella callback audio**, dopo l'equalizzatore e prima del volume:
dopo l'equalizzatore perché una curva che alza i bassi si deve vedere, prima del
volume perché uno spettro che si abbassa con la manopola descrive la manopola.
Passano da un terzo anello senza lucchetti — il crate ne aveva già due — e una
trasformata radix-2 da 4096 punti li riduce alle **dieci bande
dell'equalizzatore**, così `eq-bars` ed `eq-slider` descrivono la stessa cosa.

Tre decisioni che si vedono:

- **Trenta eventi al secondo, contro i quattro della posizione.** Non è una
  contraddizione con §9: la posizione va a quattro perché la finestra la sa
  interpolare, e fra un colpo e l'altro il tempo passa da solo. Le bande no.
- **Si somma l'energia dell'ottava, non se ne fa la media.** Le bande d'ottava
  hanno larghezza proporzionale: due bin per i 31 Hz, novecento per i 16 kHz.
  Mediando, gli acuti restavano a zero anche su un pezzo che ne è pieno — si
  vedeva, ed è così che è stato trovato.
- **`prefers-reduced-motion` non spegne lo spettro: lo ferma.** Le barre
  restano vere e si ridisegnano quattro volte al secondo invece di inseguire il
  fotogramma. Una preferenza di accessibilità non si aggira accendendo proprio
  la cosa che vieta.

Acceso e spento da un comando, in tutti e due i posti: spento, la callback non
scrive nell'anello e il filo non manda l'evento. Chi non guarda non paga.

---

## 7. La tastiera

Tutte in `apps/desktop/src/tastiera.ts`, in un file solo — una scorciatoia
sparsa nel componente che la usa è una scorciatoia che nessuno sa che esiste, e
che può collidere con un'altra senza che niente lo dica.

| Tasto | Cosa fa |
|---|---|
| `Spazio` | Play / pausa |
| `/` · `Ctrl+F` | Porta il fuoco nella ricerca |
| `←` `→` | Sposta di 5 s nel brano |
| `F` | Apre e chiude In riproduzione |
| `Esc` | Esce dal campo → chiude lo schermo intero → chiude il menù → annulla la selezione → svuota la ricerca → chiude album/artista |
| `Alt` + `↑` `↓` | Riordina la riga della coda che ha il fuoco |
| `I` | Accende e spegne la sonda, nello Studio |
| `←` `→` in un segmentato | Si muove fra le linguette senza uscire dal gruppo |

**Non mentre si scrive.** Il controllo è sul bersaglio dell'evento, non su uno
stato dell'applicazione: l'unica fonte attendibile di «dove sta il fuoco adesso»
è il documento. `Escape` è l'eccezione voluta — in un campo pieno il primo
significato di Escape è «lascia stare».

`Alt+↑↓` esiste perché il trascinamento HTML5 **non esiste** per chi non usa il
mouse: `dragstart` nasce da un puntatore e nessuna combinazione di tasti lo
produce. Finché il riordino della coda era solo trascinabile, era una funzione
che una parte degli utenti non aveva — non «scomoda», assente.

---

## 8. Accessibilità

### I contrasti, misurati

`cargo run -p aether-skin --example contrasti`. Soglia WCAG 2.1 per il testo
normale: **4.5:1**. `contrast_ratio()` tiene conto dell'opacità — `text.3` è
bianco al 46% *sopra* la superficie, e misurarlo pieno darebbe sempre 21:1.

| Davanti | Dietro | Scuro | Chiaro |
|---|---|---:|---:|
| text.1 | surface.0 | 16.73 | 15.16 |
| text.1 | surface.1 | 16.25 | 14.27 |
| text.1 | surface.2 | 15.27 | 13.40 |
| text.2 | surface.0 | 7.30 | 6.52 |
| text.2 | surface.1 | 7.25 | 6.33 |
| text.2 | surface.2 | 7.04 | 6.13 |
| text.3 | surface.0 | 4.65 | 4.91 |
| text.3 | surface.1 | 4.68 | 4.80 |
| text.3 | surface.2 | 4.65 | 4.69 |
| accent | surface.0 | 5.97 | 5.41 |
| accent | surface.1 | 5.78 | 5.05 |
| accent | surface.2 | 5.40 | 4.71 |
| danger | surface.0 | 5.08 | 5.69 |
| danger | surface.2 | 4.59 | 4.95 |
| success | surface.2 | 9.35 | 5.19 |
| warning | surface.2 | 11.73 | 4.89 |
| **surface.0 sopra accent** | | **5.97** | **5.41** |

L'ultima riga è la regola inversa e nessun'altra la copre: il testo che sta
**sopra** l'accento è `--color-surface-0`. Se l'accento si schiarisce,
l'etichetta di un bottone primario sparisce, e la tabella di sopra non se ne
accorgerebbe — lì `surface.0` è sempre un fondo.

`surface.3` è deliberatamente fuori dalla misura: è un riempimento di
sopraelevazione (il fondo di un elemento di menù al passaggio del mouse), non un
letto di testo semantico. `danger` su `surface.3` fa 4.2:1, e il menù usa
`surface.2` per la sua riga distruttiva.

### Il resto

- **Fuoco**: `:focus-visible`, mai `:focus`. Lo stato del registro delle parti si
  chiama `Focus` e compila a `:focus-visible` — mostrarlo a ogni clic del mouse
  è il difetto che il vecchio albero evitava per disciplina e qui è per
  costruzione. E adesso c'è **una regola sola e globale**: due pixel d'accento
  con due di scostamento. Prima ce n'erano sette locali e tutto il resto —
  bottoni, pillole, linguette, righe, schede — cadeva sul contorno di serie del
  motore, cioè l'unico colore che nessuna skin poteva toccare, e per giunta
  quello che vede solo chi naviga senza mouse.
- **Il caricamento si annuncia.** I segnaposto stanno in un contenitore con
  `role="status"` e un'etichetta; i riquadri sono `aria-hidden`, perché chi
  ascolta lo schermo deve sentire «sto caricando» una volta, non dodici
  rettangoli.
- **Stato letto e stato visto vengono dallo stesso attributo.** La riga in
  riproduzione porta `aria-current`; una riga scelta porta `aria-selected`.
  `data-active` e `data-scelta` stanno **accanto** e servono alle skin, non al
  posto.
- **Le etichette non si nascondono con `display: none`.** La barra richiusa usa
  la tecnica del ritaglio: sparirebbero anche per uno screen reader, e una barra
  richiusa continua a essere una navigazione con quattro destinazioni che hanno
  un nome.
- **`roving tabindex`** nei segmentati: una sola linguetta è raggiungibile col
  tabulatore, dentro ci si muove con le frecce. Un segmentato da cinque voci
  costerebbe altrimenti cinque fermate di Tab.
- **I controlli spenti dicono perché.** `i-text` e `i-eq` in `np-screen`, e
  ReplayGain in Impostazioni, portano un `tooltip-pill` che si apre al passaggio
  **e al fuoco da tastiera**: un motivo raggiungibile solo col mouse è un motivo
  che una parte delle persone non legge mai.
- **`unicode-bidi: plaintext`** accanto a `direction: rtl` sui percorsi. Senza,
  un percorso UNC come `\\nas\archivio\Vinili` si disegnava con le due barre
  iniziali in fondo.

---

## 9. Prestazioni

Fanno parte del disegno, non vengono dopo.

- Il nucleo manda **4 eventi di posizione al secondo**; `riproduzione.ts`
  interpola a 50 ms. Nessun componente deve chiedere niente a quel ritmo — e
  fino a poco fa **tutti** lo facevano. La posizione interpolata era uno
  `useState` dentro `App`, quindi ogni colpo rifaceva `App` per intero: la
  testata, il corpo con le duecento righe dell'elenco, e `contesto`, il cui
  `useMemo` aveva `posizioneMs` fra le dipendenze e non serviva a niente. React
  non riscriveva il DOM, ma riconciliava duecento righe venti volte al secondo
  per muovere una barra. Ora la posizione vive in un archivio esterno letto con
  `useSyncExternalStore`, e la legge **una foglia sola**: `Scrubber`. Misurato
  con un contatore in `RigaBrano`: da ~41 000 disegni in otto secondi di musica
  a **zero**.
- **La libreria arriva a pagine, e prima non arrivava affatto.** Duecento brani,
  quattrocento album, e i preferiti presi chiedendo duemila righe e filtrandole
  nel browser: il brano duecentouno non era raggiungibile da nessuna vista. Ora
  `usePagine` accoda le pagine quando una sentinella in fondo all'elenco entra
  in vista, con seicento pixel di anticipo perché lo scorrimento non si fermi ad
  aspettare. I preferiti sono una query (`list_liked`), gli album di un artista
  un'altra (`albums_by_artist`), e «N risultati» è un `COUNT(*)` invece della
  lunghezza della prima pagina — prima diceva «60» per una ricerca che ne aveva
  trecento.
- `useRigheCoda` usa `coda.join(",")` come chiave d'effetto: ogni evento di
  stato porta un array nuovo con dentro gli stessi numeri, e senza quella chiave
  la coda rileggerebbe le sue righe quattro volte al secondo.
- La scansione annuncia ogni 25 file, non a ogni file: un evento per ognuno di
  1421 file inonderebbe il canale IPC per muovere una barra di meno di un pixel.
- La validazione dello Studio ha 120 ms di respiro. Sotto, ogni carattere è un
  giro di parse più compile; sopra, l'anteprima resta indietro rispetto alle
  dita.
- Il budget di costo di una superficie è **10**, cioè esattamente un
  `backdrop-filter`. Lo Studio lo mostra mentre lo si spende, non alla
  validazione: un contatore che compare dopo arriva quando la superficie è già
  costruita.
- **Dove si sfoca, e dove no.** Ci sono due sole sfocature in tutta la finestra,
  e tutte e due stanno su un'immagine **ferma**: l'ambiente dietro la copertina
  e l'alone sotto di essa. Un livello sfocato che non cambia si rasterizza una
  volta e poi si compone, e cambia solo quando cambia il brano. Quel che non c'è
  è il `backdrop-filter` sotto il lettore e sotto la coda: sono le due superfici
  che stanno sopra la cosa che scorre di più, e lì una sfocatura è un ricalcolo
  per fotogramma. Il gradiente più il filo alto danno quasi lo stesso occhio a
  costo zero — è la ragione per cui `--luce` e `--filo` esistono.
- **La sfocatura chiede la miniatura.** L'ambiente usa il file da 160 pixel anche
  quando la copertina accanto è l'originale: sotto un raggio di sessantaquattro
  i dettagli di un'immagine da mille sono byte letti dal disco per essere
  buttati.

---

## 10. Le due skin

**Plain** è il riferimento, e resta quel che si dichiara: scuro, sobrio, senza
effetti. Non ridipinge nessuna parte — cambia solo token — ed è precisamente
questo che la rende utile come collaudo del formato.

**Sala** (`core/aether-skin/skins/sala.json`) è l'altra faccia, e la ragione per
cui il registro delle parti esiste. È la sala d'ascolto: neri **caldi** invece
del quasi-blu di Plain, ottone come accento, il rosso di un'etichetta sul cuore —
che è finalmente il motivo per cui `color.accent.like` è un token separato — e
raggi più stretti, perché è apparecchiatura e non scheda. Il carattere da
display è il monospaziato: gli stessi tre file impacchettati, un'altra faccia.

Spende in un posto solo. Il motivo `lampada` è una sfumatura radiale d'ottone in
alto a sinistra, ed è **l'unica sorgente della stanza**: la usa
`ambient-backdrop`, e da lì tinge la terza colonna e lo schermo intero.
`app-shell`, `player-shell`, `section-card` e `play-btn-primary` prendono un
gradiente ciascuno, `np-screen` una vignettatura; tutto il resto — bordi, colori
di testo, pesi, spaziature fra le lettere — non costa niente. Totale **9 su 10**,
col budget che è la ragione per cui la disciplina c'è: la prima stesura ne
spendeva 16 e metà di quella spesa era rumore.

Tutte le coppie di contrasto passano in tutti e due i temi
(`cargo run -p aether-skin --example contrasti -- core/aether-skin/skins/sala.json`).

Non essendo incorporata come Plain, si installa: `sala.aeskin` accanto al
documento è il pacchetto pronto, e si trascina sulla finestra o si sceglie da
Impostazioni → Aspetto.

---

## 11. Quel che resta aperto

### Quel che è stato chiuso, e come

- **Il fotogramma scuro all'avvio con una skin chiara.** `backgroundColor` in
  `tauri.conf.json` lo dipinge il sistema operativo *prima* che esista una
  pagina. La via scelta non è riscrivere la configurazione della finestra —
  quel valore resta agganciato a `plain`, ed è giusto — ma **non mostrare la
  finestra finché non c'è il colore giusto**: nasce con `"visible": false`, e il
  comando `pronto` la mostra dopo due fotogrammi dall'arrivo di skin e tema. Una
  rete di sicurezza in Rust la mostra comunque dopo due secondi, perché
  un'applicazione invisibile sarebbe un guasto peggiore del difetto che si stava
  togliendo.
- **`blurBehind` è agganciabile.** `PartAppearance` ha ora un campo `filter`
  accanto a `clip`, letto con `EffectTarget::Filter` — che esisteva già e non
  aveva un campo che lo riferisse. E il costo entra nel budget: `effects()`
  sommava i soli sfondi, quindi una sfocatura da dieci punti sarebbe passata
  senza che niente lo dicesse.
- **Lo spettro è vero.** Vedi §6.
- **La libreria non è più tagliata.** Vedi §9.
- **L'accento segue la copertina, e il contrasto lo decide OKLCH.** Il problema
  era vero e non è cambiato: `--accent` è anche un **colore di testo** — la voce
  di navigazione attiva, il titolo della riga che suona, il numero delle
  statistiche — e la tinta viva di un disco non ha nessuna garanzia di fare
  4,5:1 sulle superfici del tema. Quel che è cambiato è che adesso c'è un posto
  dove risolverlo invece di sperarci: `aether-skin::dinamico` porta la tinta in
  OKLCH e la fa scorrere **in chiarezza**, in entrambe le direzioni a partire da
  quella della copertina, fino alla prima che regge la soglia su tutte le
  superfici di `DIETRO` *e* per il testo che ci va sopra. La tonalità e il croma
  non si toccano — il colore resta riconoscibilmente quello del disco — e se
  nessuna chiarezza passa la risposta è `null` e vince l'accento della skin.
  Tre conseguenze che valgono più della meccanica:
  - **La soglia è la stessa di `check_skin`.** Le superfici sono la stessa
    lista, `CONTRASTO_MINIMO` è la stessa costante. Un accento scritto dalla
    copertina supera esattamente le prove che deve superare un accento scritto a
    mano: non ci sono due definizioni di «accento leggibile» nello stesso
    programma, ed è la ragione per cui il taglio sta in Rust e non nella
    finestra.
  - **La tinta si estrae in Rust, non su canvas.** `aether-app::tinta` legge la
    miniatura già sul disco. La strada della canvas avrebbe imposto un
    `Access-Control-Allow-Origin` sul protocollo `aether-cover`, cioè allargare
    per una decorazione un contratto tenuto stretto apposta, e avrebbe messo
    venticinquemila pixel sul filo dell'interfaccia a ogni cambio di brano.
  - **Si scrivono solo i quattro token della famiglia**, con l'alfa che la skin
    aveva già dichiarato, e solo se la skin dichiara `dynamicAccent: true`. Chi
    dice di no non lo riceve nemmeno con la preferenza accesa, e l'interruttore
    in Impostazioni lo dice invece di fingere.

### Quel che resta davvero aperto

- **ReplayGain non ha un comando.** `aether-play` legge i tag e applica il
  guadagno — `Motore::replaygain` esiste e il motore parte con la correzione
  accesa — ma l'IPC non lo espone, quindi non si può spegnere. Il controllo c'è,
  spento, con la sua ragione.
- **I testi non si leggono.** `tracks.lyrics` è popolata da `metadata.rs`,
  nessun comando la interroga. `lyrics-screen` e `lyric-line` restano parti
  registrate che nessuno emette, e il bottone in `np-screen` è spento con la sua
  ragione. È l'unico dei tre controlli spenti che resta tale per intero.
- **La cronologia si scrive e non si legge.** `listening_history` è popolata,
  nessun comando la interroga. La linguetta nella colonna c'è, spenta.
- **Il telefono.** `bottom-nav` e `platforms.mobile` restano nel registro. Il
  disegno di questa finestra non li usa e non li tocca.

### Una voce che era sbagliata

Questo documento elencava **`WarningKind::UnusedPattern` come non producibile**,
e non era vero: `{"$pattern": "nome"}` è la forma che riferisce un motivo da una
parte, `patterns_used()` la conta anche dentro gli stati, e la variante scatta.
La spiegazione per esteso sta in `document.rs` accanto alla variante. La regola
in testa a questo documento dice che quando una tabella e il codice divergono ha
ragione il codice: qui aveva ragione il codice.
