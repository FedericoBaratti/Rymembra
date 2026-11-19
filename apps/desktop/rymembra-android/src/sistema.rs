//! Il resto del sistema: i permessi, le cartelle, i documenti, la rete locale.
//!
//! # Le funzioni che aspettano una persona
//!
//! [`chiedi_permessi`], [`scegli_cartella`], [`apri_documento`] e
//! [`salva_documento`] aprono una schermata di sistema e tornano quando chi
//! guarda ha risposto. Kotlin le apre sul filo principale e le aspetta lì:
//! **non** vanno chiamate da quel filo, che è quello dei comandi sincroni della
//! finestra. Chi le chiama è un comando `async`, o un filo suo.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Errore, chiama};

/// Che cosa l'app può leggere, e se può mostrare notifiche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permessi {
    /// «Accesso a tutti i file» (Android 11+), o il vecchio permesso di
    /// lettura e scrittura (fino ad Android 10): i brani, ma anche i `.lrc`, i
    /// `.cue` e le copertine accanto, i `.lrc` dei testi sincronizzati e le
    /// eliminazioni.
    pub tutti_i_file: bool,
    /// I file audio (`READ_MEDIA_AUDIO`, o la lettura dell'archivio fino ad
    /// Android 12).
    pub audio: bool,
    /// Le immagini, per le copertine accanto ai brani.
    pub immagini: bool,
    /// Le notifiche: senza, la notifica del lettore non si vede (la musica
    /// suona lo stesso).
    pub notifiche: bool,
}

/// Lo stato dei permessi adesso. Non chiede niente a nessuno.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn permessi() -> Result<Permessi, Errore> {
    chiama("sistema.permessi", &())
}

/// Chiede i permessi per leggere la musica che si possono chiedere con un
/// dialogo, e torna con lo stato dopo la risposta. Le notifiche no: Kotlin le
/// chiede al primo bisogno vero — «Scarica qui», l'attesa di un consenso —
/// e non con questi.
///
/// Bloccante: vedi il preambolo del modulo.
///
/// # Errori
///
/// Se il ponte non risponde, o se non c'è un'attività da cui chiedere.
pub fn chiedi_permessi() -> Result<Permessi, Errore> {
    chiama("sistema.chiediPermessi", &())
}

/// Apre la pagina delle impostazioni di Android dove si concede «Accesso a
/// tutti i file». Non aspetta: il permesso si rilegge con [`permessi`] quando
/// l'app torna davanti.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn chiedi_tutti_i_file() -> Result<(), Errore> {
    chiama("sistema.chiediTuttiIFile", &())
}

/// La cartella «Music» della memoria condivisa: `/storage/emulated/0/Music`
/// sulla maggior parte dei telefoni.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn cartella_musica() -> Result<PathBuf, Errore> {
    chiama::<String>("sistema.cartellaMusica", &()).map(PathBuf::from)
}

/// La cartella scelta nel selettore di sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cartella {
    /// Una cartella della memoria del telefono o di una scheda SD, col suo
    /// percorso vero.
    Percorso(PathBuf),
    /// Una cartella di un altro fornitore — «Download», un'app cloud che offre
    /// cartelle nel selettore —, che non ha un percorso da camminare. Il
    /// fornitore, per il diario.
    Fuori(String),
}

#[derive(Deserialize)]
struct Scelta {
    #[serde(default)]
    percorso: Option<String>,
    #[serde(default)]
    fuori: Option<String>,
}

/// Fa scegliere una cartella con il selettore di sistema. `Ok(None)` se chi
/// guarda ha annullato.
///
/// Il selettore dà un URI di albero (`content://…/tree/primary:Music`); qui
/// arriva tradotto nel percorso vero (`/storage/emulated/0/Music`), perché la
/// scansione legge i file con le funzioni di sempre. Una cartella che non ha un
/// percorso non è un errore ma [`Cartella::Fuori`]: è una scelta che il
/// selettore permette, e chi chiama la dice con una frase sua.
///
/// Bloccante: vedi il preambolo del modulo.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn scegli_cartella() -> Result<Option<Cartella>, Errore> {
    let scelta: Option<Scelta> = chiama("sistema.scegliCartella", &())?;
    Ok(scelta.map(|scelta| match (scelta.percorso, scelta.fuori) {
        (Some(percorso), None) => Cartella::Percorso(PathBuf::from(percorso)),
        (_, fuori) => Cartella::Fuori(fuori.unwrap_or_default()),
    }))
}

#[derive(Serialize)]
struct Tipi<'a> {
    tipi: &'a [String],
}

/// Fa scegliere un documento da aprire, e ne restituisce una **copia** nella
/// cache dell'app. `Ok(None)` se chi guarda ha annullato.
///
/// Una copia, perché il selettore dà un URI `content://` che solo Android sa
/// leggere, e chi lo usa — l'installazione di una skin, l'importazione di un
/// profilo — legge un file. La copia si cancella da sé con la cache.
///
/// `tipi` sono tipi MIME (`application/zip`, `*/*`); vuoto vuol dire tutti.
///
/// Bloccante: vedi il preambolo del modulo.
///
/// # Errori
///
/// Se il ponte non risponde, o se il documento non si copia.
pub fn apri_documento(tipi: &[String]) -> Result<Option<PathBuf>, Errore> {
    chiama::<Option<String>>("sistema.apriDocumento", &Tipi { tipi })
        .map(|scelta| scelta.map(PathBuf::from))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Salvataggio<'a> {
    nome: &'a str,
    tipo: &'a str,
    sorgente: &'a str,
}

/// Fa scegliere dove salvare un documento, e ci copia `sorgente`. `Ok(false)`
/// se chi guarda ha annullato.
///
/// Bloccante: vedi il preambolo del modulo.
///
/// # Errori
///
/// Se il ponte non risponde, se `sorgente` non è un percorso leggibile, o se la
/// copia non riesce.
pub fn salva_documento(nome: &str, tipo: &str, sorgente: &Path) -> Result<bool, Errore> {
    let sorgente = sorgente
        .to_str()
        .ok_or_else(|| Errore::nuovo("il percorso da salvare non è testo"))?;
    chiama(
        "sistema.salvaDocumento",
        &Salvataggio {
            nome,
            tipo,
            sorgente,
        },
    )
}

#[derive(Serialize)]
struct Percorsi<'a> {
    percorsi: &'a [String],
}

/// Per ogni percorso, nello stesso ordine, se il Cestino del sistema lo può
/// prendere: da Android 11, e solo i file che `MediaStore` conosce — audio,
/// immagini, video. Un `.cue`, o un file in una cartella con `.nomedia`, no.
///
/// Non aspetta niente.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn cestinabili(percorsi: &[String]) -> Result<Vec<bool>, Errore> {
    chiama("sistema.cestinabili", &Percorsi { percorsi })
}

/// Manda i file nel Cestino del sistema, da cui si recuperano per trenta
/// giorni con l'app File. `Ok(false)` se chi guarda ha detto di no al dialogo
/// del sistema: allora non si è mosso niente.
///
/// Tutti insieme, con un dialogo solo. Ogni percorso deve essere fra quelli che
/// [`cestinabili`] ha detto di sì.
///
/// Bloccante: vedi il preambolo del modulo.
///
/// # Errori
///
/// Se il ponte non risponde, se non c'è un'attività, o se un file non è
/// nell'indice dei file multimediali.
pub fn cestina(percorsi: &[String]) -> Result<bool, Errore> {
    chiama("sistema.cestina", &Percorsi { percorsi })
}

/// La cartella della cache dell'app. Il `FileProvider` ne sa mostrare alle
/// altre app due sottocartelle soltanto: `aggiornamenti` (l'APK, per
/// l'installatore) e `diario` (la copia da condividere). Vedi
/// `res/xml/file_paths.xml`.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn cartella_cache() -> Result<PathBuf, Errore> {
    chiama::<String>("sistema.cartellaCache", &()).map(PathBuf::from)
}

/// Che cosa è successo consegnando un APK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Installazione {
    /// L'installatore di Android è aperto: decide chi usa il telefono.
    Installatore,
    /// Rymembra non può ancora installare app: è aperta la pagina delle
    /// impostazioni dove lo si concede.
    Permesso,
    /// L'APK è questa app, ma firmato con un'altra chiave: Android non lo
    /// installerebbe sopra di lei, e Kotlin l'ha già cancellato. È l'esito di
    /// una copia di prova (la chiave di debug) davanti all'APK pubblico, e non
    /// un guasto: chi chiama se lo ricorda e lo dice.
    AltraChiave,
}

/// Quale distribuzione è questa copia: `github` (l'APK delle release, con
/// l'aggiornatore) o `play` (il Google Play Store, senza aggiornatore e senza il
/// permesso di installare app). La dice la costante di build
/// `BuildConfig.DISTRIBUZIONE`, che `app/build.gradle.kts` imposta dalla
/// proprietà `distribuzione`: così un solo `.so` serve tutte e due.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn distribuzione() -> Result<String, Errore> {
    chiama("sistema.distribuzione", &())
}

/// Com'è andato il controllo del certificato di un APK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlloApk {
    /// È questa app, firmata come questa app: si può consegnare.
    Buono,
    /// È questa app ma firmata con un'altra chiave, e Kotlin l'ha già
    /// cancellato: vedi [`Installazione::AltraChiave`].
    AltraChiave,
}

/// Controlla il certificato di un APK della cartella `aggiornamenti` **senza**
/// consegnarlo all'installatore: il passo «Verifico» dell'aggiornamento, che
/// sul telefono si ferma a «Installa». Con l'APK sbagliato — un altro pacchetto,
/// un archivio che non si legge — Kotlin lo cancella e risponde con un errore.
///
/// # Errori
///
/// Se il ponte non risponde, se il percorso non è testo o non sta in quella
/// cartella, e se l'APK non si legge o è un altro pacchetto. Non vuole un'attività:
/// il controllo finisce quando finisce il download, anche con l'app in secondo piano.
pub fn controlla_apk(percorso: &Path) -> Result<ControlloApk, Errore> {
    let percorso = percorso
        .to_str()
        .ok_or_else(|| Errore::nuovo("il percorso dell'APK non è testo"))?;
    chiama("sistema.controllaApk", &Apk { percorso })
}

/// Le impronte SHA-256 dei certificati con cui è firmata la Rymembra
/// installata, in esadecimale minuscolo: quelle che un aggiornamento deve
/// avere perché Android lo installi sopra. Vuoto se Android non le dice.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn firmatari() -> Result<Vec<String>, Errore> {
    chiama("sistema.firmatari", &())
}

#[derive(Serialize)]
struct Apk<'a> {
    percorso: &'a str,
}

/// Consegna un APK — un file nella cartella `aggiornamenti` della
/// [`cartella_cache`] — all'installatore di Android, dopo averlo verificato.
/// Non aspetta la risposta: l'installazione è del sistema, e se va a buon fine
/// il sistema chiude Rymembra per sostituirla.
///
/// Un APK firmato con un'altra chiave non è un errore ma
/// [`Installazione::AltraChiave`].
///
/// # Errori
///
/// Se il ponte non risponde, se non c'è un'attività, se il percorso non è
/// testo o non sta in quella cartella, e se l'APK non si legge o è un altro
/// pacchetto: allora Kotlin l'ha già cancellato.
pub fn installa_apk(percorso: &Path) -> Result<Installazione, Errore> {
    let percorso = percorso
        .to_str()
        .ok_or_else(|| Errore::nuovo("il percorso dell'APK non è testo"))?;
    chiama("sistema.installaApk", &Apk { percorso })
}

#[derive(Serialize)]
struct Vibrazione<'a> {
    tipo: &'a str,
}

/// Una vibrazione breve, quella che il telefono dà a un gesto: `"tenuta"` per
/// un tocco lungo, `"scatto"` per una soglia superata, `"conferma"` per un
/// gesto andato a buon fine. Segue l'impostazione del sistema, e non chiede
/// permessi. Non aspetta la vibrazione: la chiede al filo dell'interfaccia e
/// torna.
///
/// # Errori
///
/// Se il ponte non risponde, o se non c'è un'attività.
pub fn vibra(tipo: &str) -> Result<(), Errore> {
    chiama("sistema.vibra", &Vibrazione { tipo })
}

#[derive(Serialize)]
struct Barre {
    chiaro: bool,
}

/// Il colore delle icone nelle barre di sistema — l'ora, la batteria, la
/// barra dei gesti — che l'app disegna sotto di sé: scure se il fondo
/// dell'app è `chiaro`, chiare se no. Android le sceglie sul tema del
/// telefono, che non è quello dell'app: un'app scura su un telefono chiaro
/// aveva l'ora nera su nero. Non aspetta il filo dell'interfaccia.
///
/// # Errori
///
/// Se il ponte non risponde, o se non c'è un'attività.
pub fn barre(chiaro: bool) -> Result<(), Errore> {
    chiama("sistema.barre", &Barre { chiaro })
}

#[derive(Serialize)]
struct Indirizzo<'a> {
    indirizzo: &'a str,
}

/// Apre un indirizzo `https` o `http` nel browser del telefono, e aspetta che
/// il sistema l'abbia preso in carico.
///
/// Sul telefono è l'unica strada: `tauri-plugin-opener` lancia un programma,
/// e Android a un'app non lo permette (`Permission denied`).
///
/// # Errori
///
/// Se il ponte non risponde, se non c'è un'attività, se l'indirizzo non è web
/// o se sul telefono non c'è un browser.
pub fn apri_indirizzo(indirizzo: &str) -> Result<(), Errore> {
    chiama("sistema.apriIndirizzo", &Indirizzo { indirizzo })
}

#[derive(Serialize)]
struct Nome<'a> {
    nome: &'a str,
}

/// Il testo di un avviso legale che viaggia nell'APK, in `assets/legale/`:
/// `LICENSE.txt`, `THIRD-PARTY-NOTICES.md`, `PRIVACY.md`, `TERMS.md`,
/// `font-OFL.txt` (li mette lì il compito Gradle `avvisiLegali`). `None` se
/// l'APK non lo porta. Kotlin accetta solo quei cinque nomi.
///
/// # Errori
///
/// Se il ponte non risponde, se il nome non è uno dei cinque, o se il file
/// c'è e non si legge.
pub fn avviso_legale(nome: &str) -> Result<Option<String>, Errore> {
    chiama("legale.leggi", &Nome { nome })
}

#[derive(Serialize)]
struct Condivisione<'a> {
    percorso: &'a str,
    tipo: &'a str,
    titolo: &'a str,
}

/// Apre il foglio di condivisione di Android su un file della cartella
/// `diario` della [`cartella_cache`]: posta, messaggi, Drive, «Salva in
/// File». Non aspetta.
///
/// # Errori
///
/// Se il ponte non risponde, se non c'è un'attività, o se il percorso non è
/// testo.
pub fn condividi(percorso: &Path, tipo: &str, titolo: &str) -> Result<(), Errore> {
    let percorso = percorso
        .to_str()
        .ok_or_else(|| Errore::nuovo("il percorso da condividere non è testo"))?;
    chiama(
        "sistema.condividi",
        &Condivisione {
            percorso,
            tipo,
            titolo,
        },
    )
}

#[derive(Serialize)]
struct Acceso {
    acceso: bool,
}

/// Tiene aperta, o lascia, la ricezione del broadcast e del multicast sulla
/// Wi-Fi.
///
/// Senza, molti telefoni scartano i pacchetti mandati a tutta la rete per
/// risparmiare batteria — e sono proprio quelli con cui «Ascolta insieme»
/// trova gli altri Rymembra.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn multicast(acceso: bool) -> Result<(), Errore> {
    chiama("sistema.multicast", &Acceso { acceso })
}

#[derive(Serialize)]
struct Sveglio<'a> {
    chi: &'a str,
    acceso: bool,
}

/// Tiene il Wi-Fi sveglio per `chi` (`insieme`, `lontano`), o lo lascia.
///
/// A schermo spento il Wi-Fi si addormenta fra un pacchetto e l'altro, e
/// l'audio che arriva da un altro dispositivo si ferma ad aspettarlo. Il
/// lucchetto è uno per tutti: si lascia quando nessuno lo chiede più.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn wifi(chi: &str, acceso: bool) -> Result<(), Errore> {
    chiama("sistema.wifi", &Sveglio { chi, acceso })
}

/// Accende, o spegne, il servizio in primo piano breve che tiene vivo il
/// processo mentre si aspetta nel browser il consenso di un servizio da
/// collegare (`ConsensoInAttesa`). Il ritorno del browser arriva a un
/// indirizzo di questo telefono, e un processo congelato non lo sente.
///
/// # Errori
///
/// Se il ponte non risponde. Un servizio che il sistema non lascia partire
/// non è un errore: il consenso di solito torna lo stesso.
pub fn attesa_consenso(acceso: bool) -> Result<(), Errore> {
    chiama("sistema.attesaConsenso", &Acceso { acceso })
}

#[derive(Serialize)]
struct Scarico<'a> {
    acceso: bool,
    numero: u32,
    di: u32,
    titolo: &'a str,
}

/// Accende, aggiorna o spegne il servizio in primo piano di «Scarica qui»
/// (`ScaricoInCorso`): senza, un'app in secondo piano che non suona viene
/// congelata, e la copia di un album si ferma appena il telefono va in tasca.
/// La notifica dice il brano `numero` di `di`, e il suo `titolo`.
///
/// # Errori
///
/// Se il ponte non risponde. Un servizio che il sistema non lascia partire
/// non è un errore: lo scaricamento va avanti finché il sistema lo lascia.
pub fn scarico(acceso: bool, numero: u32, di: u32, titolo: &str) -> Result<(), Errore> {
    chiama(
        "sistema.scarico",
        &Scarico {
            acceso,
            numero,
            di,
            titolo,
        },
    )
}

#[derive(Serialize)]
struct AggiornamentoPronto<'a> {
    acceso: bool,
    versione: &'a str,
}

/// Accende, o spegne, la notifica «Pronto da installare» (`AggiornamentoPronto`
/// in Kotlin): l'APK di `versione` è scaricato e verificato, e aspetta il tocco
/// su «Installa». Toccarla riapre l'app, dov'è la scheda dell'aggiornamento.
///
/// Kotlin decide se mostrarla: non con l'app in vista (la scheda è lì), non con
/// le notifiche spente (non si chiedono), non nella distribuzione `play`. Per
/// spegnerla `versione` non conta.
///
/// # Errori
///
/// Se il ponte non risponde, o se la distribuzione non ha l'aggiornatore (è un
/// errore di chi chiama: lì non c'è nessun APK da installare).
pub fn aggiornamento_pronto(acceso: bool, versione: &str) -> Result<(), Errore> {
    chiama(
        "sistema.aggiornamentoPronto",
        &AggiornamentoPronto { acceso, versione },
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sottofondo<'a> {
    acceso: bool,
    collegati: &'a [String],
    dalla_persona: bool,
    parti_con_auto: bool,
}

/// Accende, o spegne, il servizio in primo piano di «Resta attiva in
/// sottofondo» (`Presenza`), e gli dice a chi si è collegati: la notifica
/// recita «collegata a <PC>». Si ripete a ogni giro del nodo; il servizio rifà
/// la notifica solo se la frase cambia.
///
/// `dalla_persona` è l'interruttore delle impostazioni, e non il giro: annulla
/// uno «Spegni» della notifica non ancora arrivato al nucleo, e al primo
/// «acceso» chiede le notifiche.
///
/// `parti_con_auto` è «Parti con l'auto» com'è adesso: ogni chiamata la porta,
/// perché il servizio la ricorda da quel che gli si dice, e una chiamata che la
/// tacesse — l'interruttore del sottofondo, per dire — la spegnerebbe fino al
/// giro dopo. Vale solo col sottofondo acceso.
///
/// # Errori
///
/// Se il ponte non risponde. Un servizio che il sistema non lascia partire
/// dallo sfondo non è un errore: riparte quando l'app torna in vista.
pub fn sottofondo(
    acceso: bool,
    collegati: &[String],
    dalla_persona: bool,
    parti_con_auto: bool,
) -> Result<(), Errore> {
    chiama(
        "sistema.sottofondo",
        &Sottofondo {
            acceso,
            collegati,
            dalla_persona,
            parti_con_auto,
        },
    )
}

/// Se questa build offre «Parti con l'auto»: la costante di build
/// `PARTENZA_CON_AUTO`, che la variante per Google Play mette a falso. Non cambia
/// mai nella vita del processo; non aspetta niente.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn partenza_con_auto_offerta() -> Result<bool, Errore> {
    chiama("sistema.partenzaConAuto", &())
}

/// Se «Spegni» è stato premuto sulla notifica del sottofondo e il nucleo non
/// lo sa ancora: dice sì una volta sola, poi no. Il nucleo lo scrive nelle
/// impostazioni (`lontano/filo.rs`). Non aspetta niente.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn sottofondo_spento_dalla_notifica() -> Result<bool, Errore> {
    chiama("sistema.sottofondoSpento", &())
}

/// Se Rymembra è fuori dal risparmio batteria di Android: un servizio in primo
/// piano non esenta dal Doze, e con lo schermo spento e il telefono fermo il
/// sistema sospende la rete delle app che ne restano dentro. Non aspetta.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn senza_risparmio_batteria() -> Result<bool, Errore> {
    chiama("sistema.senzaRisparmio", &())
}

/// Apre la pagina di sistema in cui si toglie il risparmio batteria a
/// Rymembra (`ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS`, e se un telefono
/// non l'ha la scheda dell'app). Non la richiesta diretta, che la policy di
/// Google Play riserva ad altri.
///
/// Bloccante: vedi il preambolo del modulo.
///
/// # Errori
///
/// Se il ponte non risponde, se non c'è un'attività, o se nessuna delle due
/// pagine si apre.
pub fn apri_risparmio_batteria() -> Result<(), Errore> {
    chiama("sistema.apriRisparmio", &())
}

/// Se l'app vede la rete di casa.
///
/// Da Android 17 un'app che si dichiara per Android 17 non la vede finché non
/// le si concede «Dispositivi nelle vicinanze»: i pacchetti verso la Wi-Fi, e
/// quelli che arrivano da lì, si fermano nel sistema senza un errore che dica
/// perché. Prima di Android 17 è sempre `true`. Non chiede niente a nessuno.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn rete_locale() -> Result<bool, Errore> {
    chiama("sistema.reteLocale", &())
}

/// Chiede la rete di casa col dialogo di Android, se non c'è già, e dice se
/// adesso c'è.
///
/// Bloccante: vedi il preambolo del modulo.
///
/// # Errori
///
/// Se il ponte non risponde, o se non c'è un'attività da cui chiedere.
pub fn chiedi_rete_locale() -> Result<bool, Errore> {
    chiama("sistema.chiediReteLocale", &())
}

/// Come si chiama questo telefono: il «Nome del dispositivo» delle
/// impostazioni di Android, o il modello.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn nome_del_telefono() -> Result<String, Errore> {
    chiama("sistema.nomeDelTelefono", &())
}

/// Su che telefono si gira.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Versione {
    /// La versione di Android come la scrive il sistema: «17», «16».
    pub android: String,
    /// Il livello delle API: 37 per Android 17.
    pub api: u32,
    /// Il modello del telefono: «A059P».
    pub modello: String,
}

/// La versione di Android e il modello del telefono, per il diario.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn versione() -> Result<Versione, Errore> {
    chiama("sistema.versione", &())
}

/// L'uscita su cui Android manda la musica adesso.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UscitaAudio {
    /// Come si chiama: il nome delle cuffie, o «Altoparlante del telefono»
    /// nella lingua del telefono.
    pub nome: String,
    /// È Bluetooth.
    pub bluetooth: bool,
    /// Esce dal telefono stesso: l'altoparlante o l'auricolare. Un ponte di prima
    /// non lo dice, e vale `false`.
    #[serde(default)]
    pub integrata: bool,
}

/// Dove Android manda la musica adesso: la risposta di `AudioManager` da
/// Android 13, la sua regola di serie prima. `None` se non c'è nessuna uscita.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn uscita_audio() -> Result<Option<UscitaAudio>, Errore> {
    chiama("sistema.uscitaAudio", &())
}

/// Manda l'app in secondo piano, come il tasto Home: la musica continua.
///
/// # Errori
///
/// Se il ponte non risponde, o se non c'è un'attività.
pub fn in_secondo_piano() -> Result<(), Errore> {
    chiama("sistema.secondoPiano", &())
}

/// Se c'è un'attività aperta, e non sta chiudendo: la schermata dell'app, in
/// cui una finestra può nascere. Non aspetta niente: si può chiamare da
/// qualunque filo, compreso il principale.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn attivita_aperta() -> Result<bool, Errore> {
    chiama("sistema.attivitaAperta", &())
}

/// Se il telefono è in uso per Rymembra: l'attività è in vista (fra
/// `onStart` e `onStop`), o la musica suona. Serve al collegamento via
/// internet, che resta raggiungibile solo allora. Non aspetta niente.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn in_uso() -> Result<bool, Errore> {
    chiama("sistema.inUso", &())
}

/// Se la rete di adesso è a consumo (i dati mobili, un hotspot): per «Solo
/// Wi-Fi». Non aspetta niente.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn rete_a_consumo() -> Result<bool, Errore> {
    chiama("sistema.reteAConsumo", &())
}

/// L'esito dello scanner del QR di «Collega un dispositivo».
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Scansione {
    /// Il link letto; `None` se chi guarda ha annullato, o se la fotocamera
    /// non c'è o non si apre.
    pub link: Option<String>,
    /// La fotocamera non è stata concessa.
    pub negato: bool,
}

/// Apre lo scanner del QR e ne aspetta l'esito. Chiede la fotocamera, se
/// non c'è ancora.
///
/// Mai dal filo principale: lo scanner si apre lì, e chi lo aspettasse da lì
/// aspetterebbe se stesso.
///
/// # Errori
///
/// Se il ponte non risponde, o se non c'è un'attività.
pub fn scansiona_qr() -> Result<Scansione, Errore> {
    chiama("sistema.scansionaQr", &())
}

/// Quanto la pagina deve stare lontana dai bordi dello schermo, in pixel CSS:
/// le barre di sistema, il ritaglio della fotocamera, la tastiera aperta.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Riquadri {
    /// La barra di stato, o il ritaglio in cima.
    pub alto: f64,
    /// A destra, in orizzontale.
    pub destra: f64,
    /// La barra dei gesti, o la tastiera se è aperta.
    pub basso: f64,
    /// A sinistra, in orizzontale.
    pub sinistra: f64,
    /// Quanto è alta la tastiera, zero se è chiusa. Già contata in `basso`:
    /// sta a parte perché la pagina, con la tastiera aperta, toglie di mezzo
    /// quel che sta in fondo. Con un valore di serie, perché un'attività di
    /// prima che non lo mandi non faccia cadere la lettura intera.
    #[serde(default)]
    pub tastiera: f64,
}

/// Gli ultimi riquadri che l'attività ha misurato.
///
/// # Errori
///
/// Se il ponte non risponde.
pub fn riquadri() -> Result<Riquadri, Errore> {
    chiama("sistema.riquadri", &())
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn il_sottofondo_parla_a_kotlin_con_i_nomi_che_kotlin_legge() {
        // `Ponte.kt` legge `acceso`, `collegati`, `dallaPersona` e
        // `partiConAuto`: un nome scritto in un altro modo da questo lato non
        // dà un errore, dà un `JSONException` sul telefono — o peggio, per
        // `optBoolean`, un `false` muto che spegne «Parti con l'auto».
        let a = serde_json::to_value(Sottofondo {
            acceso: true,
            collegati: &["PC di casa".to_owned(), "Studio".to_owned()],
            dalla_persona: true,
            parti_con_auto: true,
        })
        .unwrap();
        assert_eq!(
            a,
            serde_json::json!({
                "acceso": true,
                "collegati": ["PC di casa", "Studio"],
                "dallaPersona": true,
                "partiConAuto": true,
            })
        );
    }

    #[test]
    fn fuori_da_android_il_ponte_dice_che_non_c_e() {
        for esito in [
            sottofondo(true, &[], false, false).err(),
            partenza_con_auto_offerta().err(),
            sottofondo_spento_dalla_notifica().err(),
            senza_risparmio_batteria().err(),
            apri_risparmio_batteria().err(),
        ] {
            assert!(
                esito.is_some_and(|e| e.to_string().contains("non su Android")),
                "un comando del sottofondo che non dice «non su Android»"
            );
        }
    }
}
