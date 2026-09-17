//! Volume, equalizzatore, normalizzazione.
//!
//! I tre stadi che stanno fra i campioni decodificati e le casse, come li
//! comanda la finestra: il cursore del volume, la curva dell'equalizzatore con
//! le sue voci salvate, e il livello a cui portare tutti i brani.
//!
//! I nomi dei livelli di normalizzazione — `spento`, `basso`, `normale`,
//! `alto` — restano qui e non nel nucleo perché sono valori di filo, non di
//! dominio: il modello sotto è e resta un bersaglio in decibel, e la
//! traduzione avviene dove il filo comincia.
//!
//! Fa parte di [`crate::riproduzione`]: la regola dei due lucchetti — prima il
//! lettore, poi la libreria — è scritta là e vale anche qui.

use aether_app::playback::{self, Equalizzazione, Normalizzazione, Volume};
use aether_play::PRESET_DI_SERIE;
use serde::Serialize;

use crate::spegnimento::Emette as _;
use tauri::{Manager as _, State};

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

use super::{StatoLettore, con_lettore, manda_stato};

/// La curva dell'equalizzatore da sola.
///
/// Un evento suo invece di [`StatoRiproduzione`](super::StatoRiproduzione):
/// comporre lo stato intero richiede una lettura del brano corrente dal
/// database, e durante un trascinamento questa roba parte una dozzina di volte
/// al secondo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatoEq {
    /// I filtri sono accesi.
    pub attivo: bool,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
}

/// Una curva che si può scegliere dall'elenco.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocePreset {
    /// Come si chiama.
    pub nome: String,
    /// Quanti decibel per banda.
    pub guadagni: Vec<f32>,
    /// Viene con l'applicazione, quindi non si può cancellare.
    pub di_serie: bool,
}

/// Il livello di normalizzazione che porta questo nome.
///
/// # Perché un nome e non un numero di decibel
///
/// Il modello sotto è ed era un bersaglio in decibel, e resta quello: qui non
/// si aggiunge niente al motore, che sapeva già portare tutto a un livello
/// scelto. Quel che mancava era un modo di dirglielo.
///
/// Ai decibel non si dà però accesso dalla finestra. Un cursore da −30 a −6
/// chiederebbe a chi ascolta di sapere cos'è un LUFS per decidere, e la
/// risposta giusta per quasi tutti è una di tre. Il nome viene tradotto qui in
/// un `match` chiuso, come `ordine` in [`crate::comandi::brani`]: quel che
/// arriva dalla finestra non raggiunge mai un valore che il motore userebbe
/// senza guardarlo.
///
/// Un nome sconosciuto vale «normale», che è il riferimento dei tag.
fn normalizzazione_da_nome(nome: &str) -> Normalizzazione {
    match nome {
        // Spento conserva il bersaglio invece di azzerarlo: chi rispegne e
        // riaccende ritrova il livello che aveva scelto, non quello di serie.
        "spento" => Normalizzazione {
            attivo: false,
            bersaglio_db: playback::BERSAGLIO_PREDEFINITO_DB,
        },
        "basso" => Normalizzazione {
            attivo: true,
            bersaglio_db: playback::BERSAGLIO_BASSO_DB,
        },
        "alto" => Normalizzazione {
            attivo: true,
            bersaglio_db: playback::BERSAGLIO_ALTO_DB,
        },
        _ => Normalizzazione {
            attivo: true,
            bersaglio_db: playback::BERSAGLIO_PREDEFINITO_DB,
        },
    }
}

/// Come si chiama il livello in cui si trova la normalizzazione.
///
/// Il verso opposto di [`normalizzazione_da_nome`], e non è un `match` perché
/// il bersaglio su disco è un `f32` che passa da un taglio: confrontarlo con
/// `==` vorrebbe dire che un valore scritto da una versione precedente, o
/// limitato da `sana`, non corrisponde a nessun nome e la finestra non
/// evidenzia niente. Si prende il più vicino dei tre, che per i valori scritti
/// da qui è sempre quello esatto.
pub(super) fn nome_normalizzazione(normalizzazione: Normalizzazione) -> &'static str {
    if !normalizzazione.attivo {
        return "spento";
    }
    let scarto = |bersaglio: f32| (normalizzazione.bersaglio_db - bersaglio).abs();
    let mut nome = "normale";
    let mut minimo = scarto(playback::BERSAGLIO_PREDEFINITO_DB);
    if scarto(playback::BERSAGLIO_BASSO_DB) < minimo {
        nome = "basso";
        minimo = scarto(playback::BERSAGLIO_BASSO_DB);
    }
    if scarto(playback::BERSAGLIO_ALTO_DB) < minimo {
        nome = "alto";
    }
    nome
}

/// Il volume da solo, per l'evento `riproduzione:volume`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatoVolume {
    volume: f32,
    muto: bool,
}

/// Cambia volume e silenziamento.
///
/// **Non manda [`StatoRiproduzione`](super::StatoRiproduzione)**, come
/// [`equalizzatore`] e per la stessa ragione: arriva a ogni passo di un cursore,
/// e comporre lo stato intero voleva dire leggere il brano corrente dal database
/// per ogni passo, e ridisegnare l'applicazione intera con quel che ne usciva.
/// Parte `riproduzione:volume`, che sono due campi.
#[tauri::command]
pub fn volume(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    volume: f32,
    muto: bool,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.volume = Volume {
            volume: volume.clamp(0.0, 1.0),
            muto,
        };
        lettore
            .motore
            .volume(lettore.volume.volume, lettore.volume.muto);
        // Il disco lo aggiorna il filo dell'orologio, entro un quarto di
        // secondo. Vedi `StatoLettore::volume_da_salvare`.
        stato
            .volume_da_salvare
            .store(true, std::sync::atomic::Ordering::Relaxed);
        app.emetti(
            "riproduzione:volume",
            StatoVolume {
                volume: lettore.volume.volume,
                muto: lettore.volume.muto,
            },
        );
        Ok(())
    })
    .map_err(errore)
}

/// Cambia la curva dell'equalizzatore.
///
/// **Non manda [`StatoRiproduzione`](super::StatoRiproduzione)**, e non è una
/// svista: comporre quello stato richiede una lettura del brano corrente dal
/// database, e questo comando arriva una dozzina di volte al secondo finché un
/// cursore è sotto il dito. Parte invece `riproduzione:eq`, che sono due campi
/// e nessuna query — quanto basta perché il pannello nella barra e la pagina
/// delle impostazioni restino d'accordo fra loro.
#[tauri::command]
pub fn equalizzatore(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    guadagni: Vec<f32>,
    attivo: bool,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        lettore.motore.equalizzatore(&guadagni, attivo);
        // `playback::normalizza` fa qui il taglio che prima si andava a
        // rileggere dal database: è **la stessa** funzione che usa `save_eq`,
        // chiamata dove il valore nasce invece che dopo un giro su SQLite. La
        // finestra vede subito quel che rivedrà alla prossima apertura, e non
        // c'è più una scrittura più una rilettura per ogni pixel di cursore.
        lettore.eq = Equalizzazione {
            attivo,
            guadagni: playback::normalizza(&guadagni),
        };
        stato
            .eq_da_salvare
            .store(true, std::sync::atomic::Ordering::Relaxed);
        app.emetti(
            "riproduzione:eq",
            StatoEq {
                attivo: lettore.eq.attivo,
                guadagni: lettore.eq.guadagni.clone(),
            },
        );
        Ok(())
    })
    .map_err(errore)
}

/// Accende o spegne la normalizzazione ReplayGain.
///
/// # Cosa cambia davvero, e cosa no
///
/// Il guadagno lo applica `aether_play::guadagno` allo stadio del volume, e la
/// correzione esiste **solo per i brani che portano il tag**: su un file senza
/// `replaygain_track_gain` questo interruttore non sposta niente, in nessuna
/// delle due posizioni. È il motivo per cui il valore di serie è «acceso» —
/// sulla libreria misurata di questo progetto nessun file ha il tag, quindi
/// acceso e spento suonano identici finché non arriva un disco che ce l'ha, e
/// allora la cosa giusta da fare è rispettarlo.
///
/// Manda [`StatoRiproduzione`](super::StatoRiproduzione) e non un evento suo,
/// al contrario dell'equalizzatore: questo comando arriva quando un dito preme
/// un interruttore, cioè una volta ogni tanto, non dodici volte al secondo.
#[tauri::command]
pub fn normalizzazione(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    livello: String,
) -> Esito<()> {
    con_lettore(&stato, |lettore| {
        let voluta = normalizzazione_da_nome(&livello);
        let stato_app = app.state::<Stato>();
        // Si rilegge quel che è stato scritto invece di fidarsi di quel che è
        // arrivato: è la disciplina dell'equalizzatore, e serve perché il
        // bersaglio passa per un taglio.
        let salvata = con_libreria(&stato_app, |libreria| {
            playback::save_replaygain(&libreria.connection, voluta)?;
            playback::load_replaygain(&libreria.connection)
        })
        .unwrap_or(voluta);
        lettore
            .motore
            .replaygain(salvata.attivo, salvata.bersaglio_db);
        lettore.normalizzazione = salvata;
        manda_stato(&app, lettore);
        Ok(())
    })
    .map_err(errore)
}

/// Le curve fra cui si può scegliere: prima quelle di serie, poi le proprie.
///
/// In quest'ordine perché è quello in cui si guardano: chi apre l'elenco la
/// prima volta non ha curve sue, e chi ne ha vuole trovarle in fondo, sempre
/// nello stesso posto, invece che mescolate alfabeticamente alle altre.
#[tauri::command]
pub fn eq_preset_elenco(stato: State<'_, Stato>) -> Esito<Vec<VocePreset>> {
    let mut elenco: Vec<VocePreset> = PRESET_DI_SERIE
        .iter()
        .map(|(nome, guadagni)| VocePreset {
            nome: (*nome).to_owned(),
            guadagni: guadagni.to_vec(),
            di_serie: true,
        })
        .collect();
    let miei = con_libreria(&stato, |libreria| {
        playback::load_preset_eq(&libreria.connection)
    })
    .map_err(errore)?;
    elenco.extend(miei.into_iter().map(|p| VocePreset {
        nome: p.nome,
        guadagni: p.guadagni,
        di_serie: false,
    }));
    Ok(elenco)
}

/// Salva la curva corrente sotto un nome.
///
/// `false` se il nome era vuoto. Un nome che c'è già sostituisce quella curva:
/// la regola sta in `aether_app::playback::salva_preset`, con la sua ragione.
#[tauri::command]
pub fn eq_preset_salva(
    stato: State<'_, Stato>,
    stato_lettore: State<'_, StatoLettore>,
    nome: String,
) -> Esito<bool> {
    let guadagni =
        con_lettore(&stato_lettore, |lettore| Ok(lettore.eq.guadagni.clone())).map_err(errore)?;
    con_libreria(&stato, |libreria| {
        playback::salva_preset(&libreria.connection, &nome, &guadagni)
    })
    .map_err(errore)
}

/// Toglie una curva salvata. `false` se non ce n'era una con quel nome.
///
/// Quelle di serie non passano di qui: non stanno nel database, quindi non c'è
/// niente da togliere e la finestra non ne offre il comando.
#[tauri::command]
pub fn eq_preset_cancella(stato: State<'_, Stato>, nome: String) -> Esito<bool> {
    con_libreria(&stato, |libreria| {
        playback::cancella_preset(&libreria.connection, &nome)
    })
    .map_err(errore)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// I quattro nomi vanno e tornano.
    ///
    /// La prova che conta davvero è il ritorno: `nome_normalizzazione` non fa
    /// un confronto esatto ma prende il più vicino, e un bersaglio nuovo
    /// aggiunto in mezzo agli altri potrebbe rubare il nome a uno dei tre
    /// senza che niente smetta di compilare.
    #[test]
    fn i_livelli_di_normalizzazione_vanno_e_tornano() {
        for nome in ["spento", "basso", "normale", "alto"] {
            let livello = normalizzazione_da_nome(nome);
            assert_eq!(
                nome_normalizzazione(livello),
                nome,
                "andata e ritorno di «{nome}»"
            );
        }
    }

    /// Un nome che non conosciamo vale «normale», non un guasto.
    #[test]
    fn un_livello_sconosciuto_vale_il_riferimento() {
        let livello = normalizzazione_da_nome("fortissimo");
        assert!(livello.attivo);
        assert_eq!(nome_normalizzazione(livello), "normale");
    }

    /// Spento conserva un bersaglio valido a cui tornare.
    ///
    /// Se spegnere scrivesse uno zero, riaccendere porterebbe tutto a 0 dB —
    /// diciotto decibel sopra il riferimento, cioè un salto di volume che
    /// nessuno ha chiesto.
    #[test]
    fn spento_non_perde_il_bersaglio() {
        let spento = normalizzazione_da_nome("spento");
        assert!(!spento.attivo);
        assert!(
            spento.bersaglio_db.is_finite() && spento.bersaglio_db < 0.0,
            "spento ha lasciato un bersaglio insensato: {}",
            spento.bersaglio_db
        );
    }
}
