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

use rymembra_app::playback::{self, Equalizzazione, ModoLivello, Normalizzazione, Volume};
use rymembra_play::PRESET_DI_SERIE;
use serde::Serialize;

use crate::spegnimento::Emette as _;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, adesso_ms, con_libreria};

use super::{StatoLettore, con_lettore, in_ordine, manda_stato};

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
///
/// `modo` passa com'è: il nome dice quanto forte, non se il livello è del
/// brano o del disco, e cambiare l'uno non deve rimettere di serie l'altro.
fn normalizzazione_da_nome(nome: &str, modo: ModoLivello) -> Normalizzazione {
    let (attivo, bersaglio_db) = match nome {
        // Spento scrive il bersaglio di serie, e non zero: sul disco resta un
        // bersaglio valido, e se qualcosa riaccendesse la normalizzazione senza
        // passare dal selettore il livello non salterebbe a 0 dB. Il livello
        // scelto prima **non** si conserva: la finestra riaccende sempre con un
        // nome, e il nome il bersaglio lo riscrive.
        "spento" => (false, playback::BERSAGLIO_PREDEFINITO_DB),
        "basso" => (true, playback::BERSAGLIO_BASSO_DB),
        "alto" => (true, playback::BERSAGLIO_ALTO_DB),
        _ => (true, playback::BERSAGLIO_PREDEFINITO_DB),
    };
    Normalizzazione {
        attivo,
        bersaglio_db,
        modo,
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
        // In fila dietro agli stati, e non subito: lo stato intero parte dal
        // filo del seguito, e un evento del volume che lo sorpassasse verrebbe
        // poi cancellato da uno stato fotografato **prima** di lui — il
        // cursore che torna indietro da solo. Vedi `super::in_ordine`.
        let carico = StatoVolume {
            volume: lettore.volume.volume,
            muto: lettore.volume.muto,
        };
        in_ordine(&app, move |app| app.emetti("riproduzione:volume", carico));
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
        // In fila dietro agli stati, per la stessa ragione del volume.
        let carico = StatoEq {
            attivo: lettore.eq.attivo,
            guadagni: lettore.eq.guadagni.clone(),
        };
        in_ordine(&app, move |app| app.emetti("riproduzione:eq", carico));
        Ok(())
    })
    .map_err(errore)
}

/// Accende o spegne la normalizzazione, e ne sceglie il livello.
///
/// # Cosa cambia davvero
///
/// Il guadagno lo applica `rymembra_play::guadagno` allo stadio del volume. Fino
/// alla 2.4 la correzione esisteva **solo per i brani che portano il tag**, e
/// sulle librerie vere — dove il tag non ce l'ha quasi nessuno — acceso e
/// spento suonavano identici. Dalla 3.0 i brani senza tag li misura la passata
/// di [`crate::analisi`], e l'interruttore fa quel che dice anche su di loro,
/// man mano che vengono misurati.
///
/// Il modo — brano, disco, automatico — resta quello scritto: lo si rilegge
/// dentro lo stesso lucchetto in cui si scrive, così due comandi ravvicinati
/// non si cancellano a vicenda la metà che non hanno toccato.
///
/// Manda [`StatoRiproduzione`](super::StatoRiproduzione) e non un evento suo,
/// al contrario dell'equalizzatore: questo comando arriva quando un dito preme
/// un interruttore, cioè una volta ogni tanto, non dodici volte al secondo.
///
/// `(async)`: scrive sul database, e sul filo della finestra, con una scansione
/// che tiene la libreria, voleva dire la finestra ferma fino a otto secondi:
/// quattro per il lettore, quattro per la libreria. La scrittura stava anche
/// **dentro** il lucchetto del lettore, perché era quello a mettere in fila due
/// comandi ravvicinati; adesso li mette in fila un lucchetto loro, e il lettore
/// si prende solo per copiare il valore riletto — vedi
/// `super::cambia_preferenza`.
#[tauri::command(async)]
pub fn normalizzazione(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    livello: String,
) -> Esito<()> {
    // Si rilegge quel che è stato scritto invece di fidarsi di quel che è
    // arrivato: è la disciplina dell'equalizzatore, e serve perché il bersaglio
    // passa per un taglio. La scrittura fuori dal lucchetto del lettore, e in
    // fila con le altre preferenze: vedi `super::cambia_preferenza`.
    super::cambia_preferenza(
        &app,
        &stato,
        "normalizzazione",
        |libreria| {
            let modo = playback::load_replaygain(&libreria.connection)?.modo;
            playback::save_replaygain(
                &libreria.connection,
                normalizzazione_da_nome(&livello, modo),
            )?;
            playback::load_replaygain(&libreria.connection)
        },
        |lettore, salvata| {
            let salvata = salvata.unwrap_or_else(|_| {
                normalizzazione_da_nome(&livello, lettore.normalizzazione.modo)
            });
            lettore
                .motore
                .replaygain(salvata.attivo, salvata.bersaglio_db);
            lettore.normalizzazione = salvata;
            manda_stato(&app, lettore);
            Ok(())
        },
    )
    .map_err(errore)
}

/// Come si sceglie il livello: del brano, del disco, o da sé.
///
/// Serve al selettore delle Impostazioni, che è l'unico a chiederlo: per questo
/// non viaggia nello stato della riproduzione, come la preferenza di
/// [`super::formato_visibile`].
#[tauri::command]
pub fn normalizzazione_modo(stato: State<'_, Stato>) -> Esito<ModoLivello> {
    con_libreria(&stato, |libreria| {
        playback::load_replaygain(&libreria.connection).map(|letta| letta.modo)
    })
    .map_err(errore)
}

/// Sceglie come si prende il livello. Riporta com'è rimasto.
///
/// Scrive, rilegge e restituisce quel che è rimasto scritto, come
/// [`super::formato_visibile_scegli`]. Il resto della normalizzazione — acceso
/// e bersaglio — si rilegge nello stesso lucchetto e si riscrive com'era.
///
/// # Da quando vale
///
/// Dal prossimo brano che si apre. Il guadagno si sceglie all'apertura, e
/// quello che suona — e quello già pronto dietro di lui — se lo tengono: un
/// cambio a metà canzone sarebbe un salto di volume che nessuno ha chiesto
/// adesso, e il prossimo brano è fra pochi minuti.
#[tauri::command]
pub fn normalizzazione_modo_scegli(
    app: tauri::AppHandle,
    stato: State<'_, StatoLettore>,
    modo: ModoLivello,
) -> Esito<ModoLivello> {
    let stato_app = super::libreria_di(&app, "modo della normalizzazione").map_err(errore)?;
    let salvata = con_libreria(&stato_app, |libreria| {
        let attuale = playback::load_replaygain(&libreria.connection)?;
        playback::save_replaygain(&libreria.connection, Normalizzazione { modo, ..attuale })?;
        playback::load_replaygain(&libreria.connection)
    })
    .map_err(errore)?;
    // Il lettore può non esserci, e la scelta resta comunque scritta: la stessa
    // clemenza di `formato_visibile_scegli`.
    let _ = con_lettore(&stato, |lettore| {
        lettore.normalizzazione = salvata;
        Ok(())
    });
    Ok(salvata.modo)
}

/// A che punto è la misura del volume.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvanzamentoVolume {
    /// I brani di cui la passata si è già occupata: misurati, muti o
    /// illeggibili. Vedi `rymembra_app::intensita::avanzamento`.
    pub misurati: i64,
    /// I brani con un file, cioè quelli che la passata misura.
    pub totale: i64,
}

/// Quanti brani della libreria sono già stati misurati, su quanti.
///
/// `(async)`: due conteggi su `tracks`, che su una libreria grande sono
/// qualche decina di millisecondi, e il selettore li chiede ogni tanto mentre
/// è aperto.
#[tauri::command(async)]
pub fn normalizzazione_avanzamento(stato: State<'_, Stato>) -> Esito<AvanzamentoVolume> {
    con_libreria(&stato, |libreria| {
        rymembra_app::intensita::avanzamento(&libreria.connection, adesso_ms())
    })
    .map(|(misurati, totale)| AvanzamentoVolume { misurati, totale })
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
/// la regola sta in `rymembra_app::playback::salva_preset`, con la sua ragione.
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
            let livello = normalizzazione_da_nome(nome, ModoLivello::default());
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
        let livello = normalizzazione_da_nome("fortissimo", ModoLivello::default());
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
        let spento = normalizzazione_da_nome("spento", ModoLivello::default());
        assert!(!spento.attivo);
        assert!(
            spento.bersaglio_db.is_finite() && spento.bersaglio_db < 0.0,
            "spento ha lasciato un bersaglio insensato: {}",
            spento.bersaglio_db
        );
    }

    /// Il nome cambia quanto forte, e il modo resta quel che era.
    ///
    /// Prima del modo i due erano una cosa sola; adesso scegliere «alto» non
    /// deve rimettere «automatico» a chi aveva scelto «disco».
    #[test]
    fn il_livello_non_tocca_il_modo() {
        for nome in ["spento", "basso", "normale", "alto"] {
            assert_eq!(
                normalizzazione_da_nome(nome, ModoLivello::Album).modo,
                ModoLivello::Album,
                "«{nome}» ha cambiato il modo"
            );
        }
    }
}
