//! Quel che la riproduzione conserva in `settings`: volume, normalizzazione,
//! autoplay, dissolvenza, latenza, uscita, spettro, formato ed equalizzatore.

use rusqlite::Connection;
use rymembra_domain::errors::AppError;
use rymembra_play::{BANDE, LIMITE_DB};

use super::{
    CHIAVE_EQ, CHIAVE_EQ_PRESET, CHIAVE_FORMATO_VISIBILE, CHIAVE_REPLAYGAIN, CHIAVE_SPETTRO_BANDE,
    CHIAVE_SPETTRO_QUALITA, CHIAVE_SPETTRO_VISIBILE, CHIAVE_VOLUME, ModoLivello,
};

/// Volume e silenziamento, come stanno su disco.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Volume {
    /// Da 0 a 1.
    pub volume: f32,
    /// Silenziato.
    pub muto: bool,
}

impl Default for Volume {
    fn default() -> Self {
        // Non 1.0: un lettore che parte al massimo la prima volta è un lettore
        // che fa saltare sulla sedia.
        Self {
            volume: 0.8,
            muto: false,
        }
    }
}

/// Volume e silenziamento come stanno **nella riga**, con la curva con cui il
/// numero va letto.
///
/// # Perché la curva sta scritta accanto al numero
///
/// Fino alla 2.4 il numero era un'ampiezza: 0,5 voleva dire metà ampiezza,
/// cioè −6 dB, e metà corsa del cursore suonava quasi come il massimo. Dalla
/// 3.0 il numero è la **posizione del cursore**, e il motore la porta al cubo
/// (`rymembra_play::volume_uscita`): metà corsa è −18 dB, che è quel che
/// l'orecchio si aspetta da «a metà». Lo stesso 0,8 letto con la curva nuova
/// suonerebbe sei decibel sotto quel che si sentiva ieri — un aggiornamento
/// che abbassa il volume a tutti senza dirlo. Una riga senza `curva` è della
/// curva vecchia, e si converte leggendola nella posizione che suona uguale:
/// la radice cubica dell'ampiezza.
#[derive(serde::Serialize, serde::Deserialize)]
struct VolumeSuDisco {
    volume: f32,
    muto: bool,
    /// 0 (assente) = ampiezza, come fino alla 2.4; [`CURVA_PERCEPITA`] =
    /// posizione del cursore.
    #[serde(default)]
    curva: u8,
}

/// La curva della 3.0: il numero è la posizione del cursore.
const CURVA_PERCEPITA: u8 = 1;

/// Conserva volume e silenziamento.
pub fn save_volume(connection: &Connection, volume: Volume) -> Result<(), AppError> {
    let su_disco = VolumeSuDisco {
        volume: volume.volume,
        muto: volume.muto,
        curva: CURVA_PERCEPITA,
    };
    let json = serde_json::to_string(&su_disco).unwrap_or_else(|_| "{}".to_owned());
    crate::settings::write(connection, CHIAVE_VOLUME, &json)
}

/// Rilegge volume e silenziamento.
///
/// Una riga scritta prima della 3.0 si converte alla curva nuova in lettura, e
/// non si riscrive: la conversione è la stessa ogni volta, e la prima
/// [`save_volume`] la fissa. Così anche un profilo esportato da una 2.4 e
/// importato qui arriva al volume a cui suonava.
pub fn load_volume(connection: &Connection) -> Result<Volume, AppError> {
    let Some(json) = crate::settings::read(connection, CHIAVE_VOLUME)? else {
        return Ok(Volume::default());
    };
    Ok(serde_json::from_str::<VolumeSuDisco>(&json)
        .map(|v| {
            let volume = if v.volume.is_finite() {
                v.volume.clamp(0.0, 1.0)
            } else {
                Volume::default().volume
            };
            Volume {
                volume: if v.curva == CURVA_PERCEPITA {
                    volume
                } else {
                    volume.cbrt()
                },
                muto: v.muto,
            }
        })
        .unwrap_or_default())
}

// ── la normalizzazione ──────────────────────────────────────────────────────

/// La normalizzazione ReplayGain, come sta su disco.
///
/// Assente vale **accesa**, ed è l'unico valore di serie che qui non si sceglie
/// ma si constata: il motore parte con `replaygain_attivo: true` da quando
/// esiste, e questa chiave arriva dopo. Scrivere `false` come valore di serie
/// vorrebbe dire che la prima apertura dopo l'aggiornamento spegne di nascosto
/// una cosa che era accesa — un cambiamento di volume che nessuno ha chiesto e
/// che nessuna schermata spiega.
///
/// Fino alla 2.4 lasciarla accesa su una libreria senza tag non cambiava
/// niente: senza `replaygain_db` la correzione era esattamente 1.0. Dalla 3.0
/// la passata d'analisi misura ogni brano ([`crate::intensita`]), e accesa
/// vuol dire davvero tutti i brani allo stesso volume. Chi vuole i dischi come
/// sono stati masterizzati la spegne.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Normalizzazione {
    /// Il guadagno si applica.
    pub attivo: bool,
    /// A quanti LUFS si porta tutto. Il riferimento dei tag è −18.
    pub bersaglio_db: f32,
    /// Livello del brano o del disco. Assente nelle righe scritte prima della
    /// 3.0, e allora vale il suo valore di serie.
    #[serde(default)]
    pub modo: ModoLivello,
}

impl Default for Normalizzazione {
    fn default() -> Self {
        Self {
            attivo: true,
            bersaglio_db: BERSAGLIO_PREDEFINITO_DB,
            modo: ModoLivello::default(),
        }
    }
}

/// Il riferimento a cui i tag ReplayGain sono misurati.
///
/// A −18 il tag si usa com'è scritto; è il valore del riferimento originale, ed
/// è la ragione per cui è questo e non −14 (lo standard delle piattaforme di
/// streaming, che qui vorrebbe dire alzare tutto di quattro decibel).
pub const BERSAGLIO_PREDEFINITO_DB: f32 = -18.0;

/// Il bersaglio «basso», per chi ascolta di notte o in cuffia.
///
/// Cinque decibel sotto il riferimento. Non è «più silenzioso» nel senso del
/// volume — quello ha già il suo cursore — ma un punto di arrivo più basso a
/// cui *tutti* i brani vengono portati: la differenza si sente su una
/// scaletta che mescola un disco degli anni Ottanta e una rimasterizzazione
/// recente, dove la seconda arriva molto più forte del primo.
pub const BERSAGLIO_BASSO_DB: f32 = -23.0;

/// Il bersaglio «alto», quello delle piattaforme di streaming.
///
/// È il valore a cui normalizzano Spotify e gli altri, ed è il motivo per cui
/// una libreria locale suona più piano di loro a parità di cursore. Alzando di
/// quattro decibel sopra il riferimento dei tag, i brani già forti arrivano
/// oltre lo zero: la correzione finale in `rymembra_play::guadagno` è tagliata a
/// +12 dB e la conversione di `rymembra_play::uscita` satura invece di
/// avvolgere, quindi il caso peggiore è una compressione udibile, non un
/// rumore. Chi ha una libreria di dischi rumorosi tiene «normale».
pub const BERSAGLIO_ALTO_DB: f32 = -14.0;

/// I limiti oltre cui un bersaglio non è più una preferenza ma un guasto.
///
/// Gli stessi che `rymembra_play::guadagno` applica alla correzione finale: qui
/// per non scrivere su disco un valore che poi verrebbe tagliato in silenzio,
/// lasciando la finestra a mostrare un numero che non è quello che si sente.
const BERSAGLIO_MIN_DB: f32 = -30.0;
pub(super) const BERSAGLIO_MAX_DB: f32 = -6.0;

/// Conserva la scelta sulla normalizzazione.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_replaygain(
    connection: &Connection,
    normalizzazione: Normalizzazione,
) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_REPLAYGAIN, &sana(normalizzazione))
}

/// Rilegge la scelta sulla normalizzazione.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// [`Normalizzazione::default`], cioè accesa: la stessa regola del resto del
/// modulo.
pub fn load_replaygain(connection: &Connection) -> Result<Normalizzazione, AppError> {
    Ok(
        crate::settings::read_json::<Normalizzazione>(connection, CHIAVE_REPLAYGAIN)?
            .map(sana)
            .unwrap_or_default(),
    )
}

// ── la coda che non finisce ─────────────────────────────────────────────────

/// Se a coda esaurita si continua da soli.
pub const CHIAVE_AUTOPLAY: &str = "player.autoplay";

/// Conserva la scelta sull'autoplay.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_autoplay(connection: &Connection, attivo: bool) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_AUTOPLAY, &attivo)
}

/// Legge la scelta sull'autoplay.
///
/// Assente vale **spento**, ed è il contrario della normalizzazione qui sopra
/// per una ragione precisa: la normalizzazione descriveva quel che il motore
/// già faceva, questo aggiunge un comportamento che prima non c'era. Un
/// aggiornamento che accendesse l'autoplay da sé farebbe partire musica che
/// nessuno ha chiesto, magari a notte fonda, in una casa in cui l'ultimo album
/// era finito apposta.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_autoplay(connection: &Connection) -> Result<bool, AppError> {
    Ok(crate::settings::read_json::<bool>(connection, CHIAVE_AUTOPLAY)?.unwrap_or(false))
}

// ── un brano dentro l'altro ─────────────────────────────────────────────────

/// Quanti secondi dura la sovrapposizione fra un brano e il successivo.
pub const CHIAVE_CROSSFADE: &str = "player.crossfade";

/// Il massimo che si può chiedere, in secondi.
///
/// Dodici come Spotify, e non per imitazione: oltre i dodici secondi la
/// sovrapposizione dura più della coda di quasi ogni brano, e quel che si
/// sente non è più un passaggio ma due canzoni suonate insieme. Il limite è
/// anche una difesa del motore — la finestra di dissolvenza va tenuta in
/// memoria e confrontata con la durata del brano, e un valore assurdo
/// significherebbe una dissolvenza che comincia prima della metà.
pub const CROSSFADE_MASSIMO_S: u64 = 12;

/// Riporta i secondi dentro il consentito.
fn sano_crossfade(secondi: u64) -> u64 {
    secondi.min(CROSSFADE_MASSIMO_S)
}

/// Conserva la durata della dissolvenza incrociata, in secondi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_crossfade(connection: &Connection, secondi: u64) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_CROSSFADE, &sano_crossfade(secondi))
}

/// Legge la durata della dissolvenza. Assente vale **zero**, cioè spenta.
///
/// Spenta di serie per la stessa ragione dell'autoplay qui sopra: fino a ieri
/// il passaggio fra due brani era esatto al campione, ed è una qualità che chi
/// ascolta un album ha scelto Rymembra per avere. Un aggiornamento che
/// accendesse la dissolvenza da sé sovrapporrebbe le tracce di un disco
/// pensato per non averne — cioè romperebbe il gapless senza che nessuno
/// l'abbia chiesto.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_crossfade(connection: &Connection) -> Result<u64, AppError> {
    Ok(
        crate::settings::read_json::<u64>(connection, CHIAVE_CROSSFADE)?
            .map(sano_crossfade)
            .unwrap_or(0),
    )
}

// ── quanto il suono esce dopo ───────────────────────────────────────────────

/// Di quanti millisecondi la catena d'uscita ritarda il suono, dichiarati a mano.
///
/// # Perché `audio.` e non `player.`
///
/// Perché non descrive il lettore: descrive **questa uscita su questo computer**.
/// Le chiavi `player.*` sono scelte di chi ascolta — il volume, la curva, se
/// l'autoplay continua — e viaggiano nel profilo; questa è una proprietà del
/// cavo, del driver e del DAC che ci sono attaccati, e cambia quando cambia
/// l'hardware, non quando cambia il gusto.
pub const CHIAVE_LATENZA: &str = "audio.latenza_ms";

/// Il massimo che si può dichiarare, in valore assoluto.
///
/// Mezzo secondo. Non è un limite tecnico — una catena Bluetooth scadente ci
/// arriva — è il limite oltre il quale la compensazione smette di correggere e
/// diventa un'altra cosa: a mezzo secondo il cursore sta già mezzo pollice dietro
/// la musica, e chi continuasse ad alzare sarebbe uno che sta cercando di
/// risolvere un problema diverso.
///
/// Si applica **ai due versi**: negativo anticipa la posizione raccontata, ed è
/// il verso che serve a chi trova che i testi arrivino tardi comunque.
pub const LATENZA_MASSIMA_MS: i64 = 500;

/// Riporta i millisecondi dentro il consentito.
fn sana_latenza(ms: i64) -> i64 {
    ms.clamp(-LATENZA_MASSIMA_MS, LATENZA_MASSIMA_MS)
}

/// Conserva la latenza d'uscita dichiarata a mano, in millisecondi.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_latenza(connection: &Connection, ms: i64) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_LATENZA, &sana_latenza(ms))
}

/// Legge la latenza dichiarata. Assente vale **zero**.
///
/// Zero di serie, e non una stima: una correzione che nessuno ha chiesto
/// sposterebbe il cursore di tutti per sistemare l'uscita di qualcuno. Il pezzo
/// di catena che si può misurare lo misura già il motore
/// (`rymembra_play::uscita::annota_latenza`); questo numero esiste per il pezzo che
/// nessuno misura, e chi lo tocca lo fa guardando l'effetto.
///
/// Un valore fuori scala si stringe **in lettura** oltre che in scrittura: questa
/// riga può arrivare da un database scritto a mano o da un backup, e un numero
/// assurdo non deve poter spostare la posizione di mezz'ora.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_latenza(connection: &Connection) -> Result<i64, AppError> {
    Ok(
        crate::settings::read_json::<i64>(connection, CHIAVE_LATENZA)?
            .map(sana_latenza)
            .unwrap_or(0),
    )
}

/// La latenza dichiarata **per ogni uscita**, accanto a quella globale.
///
/// # Perché una per uscita
///
/// Perché [`CHIAVE_LATENZA`] è una sola e le catene sono tante: le casse col
/// cavo, le cuffie Bluetooth e il DAC sulla scrivania ritardano ciascuno del
/// suo, e chi passa dall'una all'altra ritrovava il numero tarato per quella di
/// prima. Ascoltare insieme su più computer lo rende un difetto e non più un
/// fastidio: lì il numero sposta **il suono**, non solo il cursore.
///
/// # Come sta scritta
///
/// Un oggetto JSON da nome d'uscita a [`LatenzaUscita`]. Il nome è quello che
/// `rymembra_play::Motore::dispositivo` dice di aver **davvero** aperto, non la
/// preferenza `player.output`: quando la scheda scelta non c'è l'apertura
/// ripiega su un'altra, e il numero va a chi sta suonando.
///
/// Sta sotto `audio.` come la globale, e per la stessa ragione resta fuori dal
/// profilo: descrive questa catena su questo computer — e in più dentro ci
/// sono i nomi delle sue schede.
pub const CHIAVE_LATENZA_PER_USCITA: &str = "audio.latenza_per_uscita";

/// Da dove viene il numero di un'uscita.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FonteLatenza {
    /// Il cursore delle impostazioni: un numero dichiarato, non misurato.
    Mano,
    /// Una taratura a orecchio, guardando il cursore contro la musica.
    Orecchio,
    /// Una misura col microfono.
    Microfono,
}

/// La latenza di un'uscita, con quel che serve a sapere quanto fidarsene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LatenzaUscita {
    /// I millisecondi, nel verso e dentro i limiti di [`CHIAVE_LATENZA`].
    pub ms: i64,
    /// Chi ha deciso il numero.
    pub fonte: FonteLatenza,
    /// Il numero è stato confermato e niente l'ha smentito da allora. Un
    /// evento che cambia la catena — le cuffie che si ricollegano con un altro
    /// codec — lo riporta a `false` senza toccare `ms`.
    pub verificata: bool,
    /// Quando è stato scritto, in millisecondi dall'epoca.
    pub quando_ms: i64,
}

/// Le latenze delle uscite, per nome. Un `BTreeMap` perché il JSON che ne esce
/// sia lo stesso a parità di contenuto: due scritture uguali, una riga uguale.
pub type LatenzePerUscita = std::collections::BTreeMap<String, LatenzaUscita>;

/// Quel che [`load_latenze_per_uscita`] ha trovato, e quel che ha dovuto
/// lasciare fuori.
///
/// Il nucleo non ha un diario, e una mappa rotta che diventa vuota senza dirlo
/// è un utente che ritrova tutte le uscite a zero senza sapere perché. I due
/// campi accanto alle voci sono il modo di dirlo alla finestra, che il diario
/// ce l'ha — e sono solo un `bool` e un numero: niente nomi, che sono le schede
/// di qualcuno.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LatenzeLette {
    /// Le voci lette, già riportate dentro i limiti.
    pub voci: LatenzePerUscita,
    /// La chiave c'era ma non era un oggetto JSON: vale come vuota.
    pub illeggibile: bool,
    /// Quante voci c'erano ma non si leggevano, e sono rimaste fuori.
    pub scartate: usize,
}

/// Rilegge le latenze per uscita. Assente vale **nessuna voce**, cioè la
/// globale per tutte.
///
/// # Una voce rotta non porta via le altre
///
/// Si legge voce per voce: un nome con un campo storto — una `fonte` che questa
/// versione non conosce, scritta da una più nuova — resta fuori da solo, e le
/// tarature delle altre uscite restano. Solo un valore che non è nemmeno un
/// oggetto vale tutto vuoto. In tutti e due i casi lo dice [`LatenzeLette`].
///
/// Come per la globale, i millisecondi si stringono **in lettura** oltre che in
/// scrittura: la riga può arrivare da un database ritoccato a mano.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Il JSON storto non ne produce
/// nessuno.
pub fn load_latenze_per_uscita(connection: &Connection) -> Result<LatenzeLette, AppError> {
    let mut lette = LatenzeLette::default();
    let Some(grezzo) = crate::settings::read(connection, CHIAVE_LATENZA_PER_USCITA)? else {
        return Ok(lette);
    };
    let Ok(voci) =
        serde_json::from_str::<std::collections::BTreeMap<String, serde_json::Value>>(&grezzo)
    else {
        lette.illeggibile = true;
        return Ok(lette);
    };
    for (nome, valore) in voci {
        match serde_json::from_value::<LatenzaUscita>(valore) {
            Ok(voce) => {
                lette.voci.insert(nome, sana_voce(voce));
            }
            Err(_) => lette.scartate = lette.scartate.saturating_add(1),
        }
    }
    Ok(lette)
}

/// Conserva la latenza di un'uscita, lasciando com'erano le altre.
///
/// Rilegge la mappa, cambia la voce di `nome` e la riscrive intera. Le voci
/// che la lettura ha scartato perché rotte se ne vanno con questa scrittura,
/// ed è voluto: una mappa illeggibile si ripara da sola alla prima taratura
/// invece di restare illeggibile per sempre.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_latenza_per_uscita(
    connection: &Connection,
    nome: &str,
    voce: LatenzaUscita,
) -> Result<(), AppError> {
    let mut voci = load_latenze_per_uscita(connection)?.voci;
    voci.insert(nome.to_owned(), sana_voce(voce));
    crate::settings::write_json(connection, CHIAVE_LATENZA_PER_USCITA, &voci)
}

/// La latenza da dare al motore che suona su `dispositivo`.
///
/// Il numero dell'uscita se c'è, altrimenti la globale. `None` — nessun motore
/// aperto, o un nome che non si sa — vale la globale anche lui.
///
/// La globale come ripiego, e non zero, perché è il numero che chi aggiorna ha
/// già tarato: prima di questa versione valeva per tutte le uscite, e
/// un'uscita che non ha ancora un numero suo continua a sentirsi come ieri.
#[must_use]
pub fn latenza_effettiva(globale: i64, mappa: &LatenzePerUscita, dispositivo: Option<&str>) -> i64 {
    sana_latenza(
        dispositivo
            .and_then(|nome| mappa.get(nome))
            .map_or(globale, |voce| voce.ms),
    )
}

/// Il cursore delle impostazioni: scrive il numero per l'uscita che suona e
/// anche come globale, e rilegge le due cose.
///
/// # Perché tutte e due
///
/// Quella dell'uscita perché è la ragione per cui la mappa esiste. La globale
/// perché il cursore di oggi è uno solo e deve continuare a valere: è il
/// ripiego di ogni uscita che un numero suo non ce l'ha ancora, e senza
/// aggiornarla le casse mai tarate continuerebbero a usare quello di ieri
/// mentre il cursore ne mostra un altro.
///
/// `uscita` è `None` quando nessun motore è aperto: allora si scrive la sola
/// globale. La voce porta `fonte: mano` e `verificata: false`, perché il
/// cursore dice quanto si **vuole** correggere, non quanto si è misurato; la
/// verifica la darà la taratura, quando ci sarà.
///
/// Restituisce la globale e la mappa come sono rimaste scritte: chi chiama ne
/// ricava la latenza per l'uscita che ha davanti con [`latenza_effettiva`] —
/// che è la stessa uscita di prima, o quella nuova se nel frattempo il motore è
/// stato riaperto.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn dichiara_latenza(
    connection: &Connection,
    uscita: Option<&str>,
    ms: i64,
    quando_ms: i64,
) -> Result<(i64, LatenzePerUscita), AppError> {
    if let Some(nome) = uscita {
        save_latenza_per_uscita(
            connection,
            nome,
            LatenzaUscita {
                ms,
                fonte: FonteLatenza::Mano,
                verificata: false,
                quando_ms,
            },
        )?;
    }
    save_latenza(connection, ms)?;
    Ok((
        load_latenza(connection)?,
        load_latenze_per_uscita(connection)?.voci,
    ))
}

/// Una voce con i millisecondi dentro i limiti della globale.
fn sana_voce(voce: LatenzaUscita) -> LatenzaUscita {
    LatenzaUscita {
        ms: sana_latenza(voce.ms),
        ..voce
    }
}

// ── da quale scheda esce il suono ───────────────────────────────────────────

/// Il nome dell'uscita audio scelta a mano. Assente: quella di sistema.
pub const CHIAVE_USCITA: &str = "player.output";

/// Conserva su quale uscita si vuole sentire Rymembra.
///
/// `None` non è «non lo so»: è la scelta esplicita «quella che usa il sistema»,
/// e va scritta come le altre — cancellare la chiave e riscriverla sono la
/// stessa cosa per chi legge, ma un `null` in `settings` dice che qualcuno c'è
/// passato, e nel dubbio è quel che si vuole trovare.
///
/// # Perché il nome e non un identificativo
///
/// Perché le scelte già salvate sono nomi: fino alla 2.4 cpal non dava altro, e
/// dalla 0.18 il suo `Device::id()` stabile serve a riconoscere il predefinito
/// fra due schede omonime, non ancora a ricordare la scelta — passarci vorrebbe
/// dire migrare le preferenze. Il prezzo è
/// scritto in testa a `rymembra_play::dispositivi` — nomi tradotti, ripetuti, che
/// un aggiornamento del driver può riscrivere — e si paga con il ripiego:
/// `dispositivi::scegli` torna al predefinito quando il nome non corrisponde a
/// niente, senza chiamarlo errore.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_uscita(connection: &Connection, id: Option<&str>) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_USCITA, &id)
}

/// Rilegge l'uscita scelta. Assente, o illeggibile, vale «quella di sistema».
///
/// Illeggibile **non** è un errore, ed è voluto: questa chiave arriva anche da
/// un database ripristinato da un backup fatto su un altro computer, dove il
/// nome scritto qui non nomina niente. Rifiutare di partire per una preferenza
/// che non si può onorare vorrebbe dire un'applicazione muta al posto di
/// un'applicazione che suona dagli altoparlanti.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn load_uscita(connection: &Connection) -> Result<Option<String>, AppError> {
    Ok(crate::settings::read_json::<Option<String>>(connection, CHIAVE_USCITA)?.flatten())
}

/// Conserva quante barre disegna lo spettro.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_spettro_bande(connection: &Connection, quante: u16) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_SPETTRO_BANDE, &sane(quante))
}

/// Rilegge quante barre disegna lo spettro.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile o fuori
/// scala vale [`rymembra_play::RISOLUZIONE_DI_SERIE`]: la stessa regola del resto
/// del modulo, e qui conta doppio perché la preferenza è disegno, non suono —
/// rifiutare di partire per una barra in più sarebbe sproporzionato.
pub fn load_spettro_bande(connection: &Connection) -> Result<u16, AppError> {
    Ok(
        crate::settings::read_json::<u16>(connection, CHIAVE_SPETTRO_BANDE)?
            .map(sane)
            .unwrap_or(rymembra_play::RISOLUZIONE_DI_SERIE),
    )
}

/// Porta un numero di barre su una delle risoluzioni che esistono.
///
/// Non un `clamp`: fra 64 e 128 non c'è niente, e scrivere 100 vorrebbe dire
/// conservare un valore che il motore poi stringe per conto suo — cioè una
/// preferenza che dice una cosa e ne fa un'altra. Si sceglie la potenza di due
/// più vicina *in rapporto*, che è il modo in cui queste scale si confrontano:
/// fra 64 e 128, il mezzo è 90, non 96.
fn sane(quante: u16) -> u16 {
    rymembra_play::RISOLUZIONI
        .into_iter()
        .min_by(|a, b| {
            let scarto = |v: u16| (f32::from(v) / f32::from(quante.max(1))).log2().abs();
            scarto(*a).total_cmp(&scarto(*b))
        })
        .unwrap_or(rymembra_play::RISOLUZIONE_DI_SERIE)
}

/// Porta un bersaglio dentro i limiti, e un valore non finito al riferimento.
fn sana(normalizzazione: Normalizzazione) -> Normalizzazione {
    Normalizzazione {
        attivo: normalizzazione.attivo,
        modo: normalizzazione.modo,
        bersaglio_db: if normalizzazione.bersaglio_db.is_finite() {
            normalizzazione
                .bersaglio_db
                .clamp(BERSAGLIO_MIN_DB, BERSAGLIO_MAX_DB)
        } else {
            BERSAGLIO_PREDEFINITO_DB
        },
    }
}

// ── lo spettro: se si vede, e quanto può costare ────────────────────────────

/// Quanto la scena dello spettro può spendere su questa macchina.
///
/// Una skin dice come la scena **appare**; questa dice quanto questo computer è
/// disposto a pagarla. Sono due domande diverse, e tenerle separate è ciò che
/// permette alla prima di viaggiare in un profilo mentre la seconda resta qui.
///
/// # Perché non c'è nessun `sane()` a mano
///
/// Perché il ritaglio esiste già, scritto una volta sola e più in alto.
/// `#[serde(rename_all = "lowercase")]` significa che nel database finiscono
/// esattamente tre parole — `"auto"`, `"alta"`, `"bassa"` — e che qualunque
/// altra cosa, `"ultra"` compreso, **non si deserializza**.
/// [`crate::settings::read_json`] dichiara già che un valore che non si
/// interpreta vale *come se non ci fosse*, e assente vale [`Qualita::Auto`].
/// Il ritaglio cade fuori da quella regola invece di essere una seconda regola
/// che qualcuno dovrà ricordarsi di tenere allineata alla prima.
///
/// Per le barre è diverso, e [`sane`] resta dov'è: là i valori validi sono otto
/// numeri fra i quali non c'è niente, e un `100` scritto da una versione con un
/// altro elenco di risoluzioni **si deserializza benissimo** — il tipo è `u16`,
/// non un'enumerazione. Là il ritaglio va scritto perché serde non lo fa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Qualita {
    /// Lo decide la scena, guardando quel che questa macchina riesce a fare.
    Auto,
    /// Il massimo che la scena sa disegnare, costi quel che costi.
    Alta,
    /// Il minimo indispensabile: ventola ferma e batteria che dura.
    Bassa,
}

impl Qualita {
    /// Il livello che porta questo nome; ogni altro nome vale [`Qualita::Auto`].
    ///
    /// È l'ingresso dal filo: la finestra manda una stringa, e una stringa può
    /// essere qualunque cosa. I nomi sono gli **stessi** che serde scrive nel
    /// database — se i due elenchi divergessero, la preferenza di ieri
    /// diventerebbe illeggibile oggi, in silenzio — e c'è una prova che lo
    /// tiene vero: `il_nome_sul_filo_e_il_nome_nel_database`.
    #[must_use]
    pub fn da_nome(nome: &str) -> Self {
        match nome {
            "alta" => Self::Alta,
            "bassa" => Self::Bassa,
            _ => Self::Auto,
        }
    }
}

/// Conserva se la scena dello spettro parte accesa.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_spettro_visibile(connection: &Connection, acceso: bool) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_SPETTRO_VISIBILE, &acceso)
}

/// Rilegge se la scena dello spettro parte accesa. Mai scelta, è spenta.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// come assente, cioè spento: la stessa regola del resto del modulo, e qui la
/// più conservativa delle due — una scena che non si accende si accende con un
/// click, una che si accende da sola su un dato storto costa una GPU a chi non
/// l'aveva chiesta.
pub fn load_spettro_visibile(connection: &Connection) -> Result<bool, AppError> {
    Ok(crate::settings::read_json::<bool>(connection, CHIAVE_SPETTRO_VISIBILE)?.unwrap_or(false))
}

/// Conserva il tetto di qualità della scena dello spettro.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_spettro_qualita(connection: &Connection, qualita: Qualita) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_SPETTRO_QUALITA, &qualita)
}

/// Rilegge il tetto di qualità. Mai scelto, o illeggibile, è [`Qualita::Auto`].
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un nome che questa versione
/// non conosce non è un errore, e la ragione sta nella carta di [`Qualita`]:
/// serde rifiuta di leggerlo, `read_json` lo tratta come assente, e assente è
/// «lo decide la scena» — che è la risposta giusta anche quando la preferenza
/// arriva da una versione che aveva un livello in più.
pub fn load_spettro_qualita(connection: &Connection) -> Result<Qualita, AppError> {
    Ok(
        crate::settings::read_json::<Qualita>(connection, CHIAVE_SPETTRO_QUALITA)?
            .unwrap_or(Qualita::Auto),
    )
}

// ── i dati tecnici del file: se si mostrano ─────────────────────────────────

/// Conserva se i dati tecnici del file si mostrano.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_formato_visibile(connection: &Connection, acceso: bool) -> Result<(), AppError> {
    crate::settings::write_json(connection, CHIAVE_FORMATO_VISIBILE, &acceso)
}

/// Rilegge se i dati tecnici del file si mostrano. Mai scelta, è accesa.
///
/// # Perché il valore di serie è l'opposto di quello dello spettro
///
/// Perché il costo è l'opposto. Una scena WebGL che si accende da sola su un
/// valore illeggibile costa una GPU a chi non l'aveva chiesta, e lì «assente
/// vale spento» è la più conservativa delle due risposte. Qui la riga costa una
/// `SELECT` su quattro colonne di una riga sola, una volta per cambio di brano:
/// il danno di un'accensione indesiderata è una riga di testo in più, e il
/// danno di uno spegnimento indesiderato è che la funzione non esiste per chi
/// non sa di doverla cercare.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// come assente, cioè acceso: la stessa regola del resto del modulo, applicata
/// al valore di serie che questa chiave ha.
pub fn load_formato_visibile(connection: &Connection) -> Result<bool, AppError> {
    Ok(crate::settings::read_json::<bool>(connection, CHIAVE_FORMATO_VISIBILE)?.unwrap_or(true))
}

// ── l'equalizzatore ─────────────────────────────────────────────────────────

/// La curva dell'equalizzatore, come sta su disco.
///
/// I guadagni in un `Vec` e non in un `[f32; BANDE]`: il giorno in cui le bande
/// cambiassero di numero, un valore scritto dalla versione di prima deve valere
/// «adattabile», non «illeggibile» — e un array di lunghezza fissa in serde è
/// esattamente la seconda cosa.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Equalizzazione {
    /// I filtri sono accesi.
    pub attivo: bool,
    /// Quanti decibel per banda, nell'ordine di `rymembra_play::CENTRI_HZ`.
    pub guadagni: Vec<f32>,
}

impl Default for Equalizzazione {
    fn default() -> Self {
        // Spento, e con la curva piatta: un lettore che alla prima apertura
        // suona già equalizzato è un lettore che suona sbagliato senza dirlo.
        Self {
            attivo: false,
            guadagni: vec![0.0; BANDE],
        }
    }
}

/// Una curva salvata dall'utente con un nome.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresetEq {
    /// Come l'ha chiamata.
    pub nome: String,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
}

/// Porta una curva alla forma giusta: [`BANDE`] valori, ognuno nei limiti.
///
/// Una curva più corta si completa con degli zeri, una più lunga si tronca, e i
/// valori non finiti valgono zero. È la stessa cautela con cui [`load_volume`]
/// taglia fra zero e uno, e per lo stesso motivo: questo valore sta in un file
/// che si può aprire e correggere a mano.
///
/// Pubblica perché la chiama anche il comando `equalizzatore`, che ha smesso di
/// scrivere e rileggere il database a ogni pixel di cursore e ha bisogno dello
/// stesso taglio senza passarci in mezzo. Averne due sarebbe averne due diverse.
pub fn normalizza(guadagni: &[f32]) -> Vec<f32> {
    (0..BANDE)
        .map(|banda| {
            let grezzo = guadagni.get(banda).copied().unwrap_or(0.0);
            if grezzo.is_finite() {
                grezzo.clamp(-LIMITE_DB, LIMITE_DB)
            } else {
                0.0
            }
        })
        .collect()
}

/// Conserva la curva dell'equalizzatore.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_eq(connection: &Connection, eq: &Equalizzazione) -> Result<(), AppError> {
    crate::settings::write_json(
        connection,
        CHIAVE_EQ,
        &Equalizzazione {
            attivo: eq.attivo,
            guadagni: normalizza(&eq.guadagni),
        },
    )
}

/// Rilegge la curva conservata.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un valore illeggibile vale
/// curva piatta e spenta, non errore.
pub fn load_eq(connection: &Connection) -> Result<Equalizzazione, AppError> {
    let Some(letto) = crate::settings::read_json::<Equalizzazione>(connection, CHIAVE_EQ)? else {
        return Ok(Equalizzazione::default());
    };
    Ok(Equalizzazione {
        attivo: letto.attivo,
        guadagni: normalizza(&letto.guadagni),
    })
}

/// Conserva l'elenco delle curve salvate.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn save_preset_eq(connection: &Connection, preset: &[PresetEq]) -> Result<(), AppError> {
    let puliti: Vec<PresetEq> = preset
        .iter()
        .map(|p| PresetEq {
            nome: p.nome.trim().to_owned(),
            guadagni: normalizza(&p.guadagni),
        })
        .filter(|p| !p.nome.is_empty())
        .collect();
    crate::settings::write_json(connection, CHIAVE_EQ_PRESET, &puliti)
}

/// Rilegge le curve salvate.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde. Un elenco illeggibile vale
/// elenco vuoto: l'utente al massimo risalva una curva, mentre un avvio che
/// fallisce non gli lascia nessun modo di correggerla.
pub fn load_preset_eq(connection: &Connection) -> Result<Vec<PresetEq>, AppError> {
    let letti: Vec<PresetEq> =
        crate::settings::read_json(connection, CHIAVE_EQ_PRESET)?.unwrap_or_default();
    Ok(letti
        .into_iter()
        .map(|p| PresetEq {
            nome: p.nome.trim().to_owned(),
            guadagni: normalizza(&p.guadagni),
        })
        .filter(|p| !p.nome.is_empty())
        .collect())
}

/// Aggiunge una curva all'elenco, o sostituisce quella che aveva già quel nome.
///
/// Sostituire e non affiancare: due voci con lo stesso nome nell'elenco sono
/// due voci che l'utente non sa distinguere, e la seconda è quasi sempre la
/// correzione della prima. Il confronto ignora maiuscole e spazi ai bordi,
/// perché «Sera» e «sera » sono lo stesso nome per chi li scrive.
///
/// Un nome vuoto non salva niente e restituisce `false`.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn salva_preset(
    connection: &Connection,
    nome: &str,
    guadagni: &[f32],
) -> Result<bool, AppError> {
    let nome = nome.trim();
    if nome.is_empty() {
        return Ok(false);
    }
    let mut elenco = load_preset_eq(connection)?;
    let nuovo = PresetEq {
        nome: nome.to_owned(),
        guadagni: normalizza(guadagni),
    };
    match elenco
        .iter_mut()
        .find(|p| p.nome.eq_ignore_ascii_case(nome))
    {
        Some(esistente) => *esistente = nuovo,
        None => elenco.push(nuovo),
    }
    save_preset_eq(connection, &elenco)?;
    Ok(true)
}

/// Toglie una curva salvata. `false` se non ce n'era una con quel nome.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn cancella_preset(connection: &Connection, nome: &str) -> Result<bool, AppError> {
    let nome = nome.trim();
    let mut elenco = load_preset_eq(connection)?;
    let prima = elenco.len();
    elenco.retain(|p| !p.nome.eq_ignore_ascii_case(nome));
    if elenco.len() == prima {
        return Ok(false);
    }
    save_preset_eq(connection, &elenco)?;
    Ok(true)
}
