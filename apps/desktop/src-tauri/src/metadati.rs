//! I brani con i metadati messi male: vederli, correggerli, chiudere la domanda.
//!
//! # La forma è quella di ogni operazione che si può rifiutare
//!
//! `piano → esegui → annulla` — la scansione, il ripristino, l'importazione di
//! un account — è già il vocabolario dell'applicazione per le operazioni su
//! cui l'utente deve poter dire di no. Qui la stessa forma diventa
//! `elenco → correggi/conferma`, e l'annullamento non serve perché non si tocca
//! niente sul disco: la correzione vive in `track_overrides` e si disfa
//! riscrivendola.
//!
//! # Perché i comandi sono quattro e non uno
//!
//! Perché le risposte che l'utente dà sono due, e sono diverse. «Questo campo è
//! sbagliato, ecco quello giusto» è [`metadati_correggi`]; «la deduzione è
//! giusta, non chiedermelo più» è [`metadati_conferma`], ed è di gran lunga la
//! più frequente — la maggior parte delle deduzioni sono corrette, e senza un
//! gesto per dirlo l'elenco non si svuoterebbe mai.

use rymembra_app::incerti::{self, TracciaIncerta};
use rymembra_app::provenienza::Correzioni;
use tauri::State;

use crate::errore::{Esito, errore};
use crate::stato::{Stato, con_libreria};

/// Quanti brani aspettano che qualcuno li guardi.
///
/// Serve alla pastiglia sul pulsante: un numero che si legge senza aprire
/// niente, come `arricchimento_stato`.
#[tauri::command]
pub fn metadati_conteggio(stato: State<'_, Stato>) -> Esito<i64> {
    con_libreria(&stato, |libreria| incerti::quanti(&libreria.connection)).map_err(errore)
}

/// Una pagina di brani da guardare.
///
/// `(async)`: legge fino a `limite` righe con il loro JSON di provenienza, e su
/// una libreria appena importata da un disco esterno quelle righe sono molte.
#[tauri::command(async)]
pub fn metadati_incerti(
    stato: State<'_, Stato>,
    offset: i64,
    limite: i64,
) -> Esito<Vec<TracciaIncerta>> {
    let (offset, limite) = crate::comandi::pagina(offset, limite);
    con_libreria(&stato, |libreria| {
        incerti::elenco(&libreria.connection, offset, limite)
    })
    .map_err(errore)
}

/// Scrive la correzione dell'utente su un brano.
///
/// `(async)`: ricostruisce gli aggregati, che su una libreria grande è la parte
/// lenta — un artista o un album appena corretti devono esistere come schede, e
/// farlo sul filo della finestra la bloccherebbe.
#[tauri::command(async)]
pub fn metadati_correggi(
    stato: State<'_, Stato>,
    id: i64,
    campi: Correzioni,
) -> Esito<Option<TracciaIncerta>> {
    // Un id di sessione non è una riga: vedi `riproduzione::ospiti`.
    let id = crate::riproduzione::riga_di_libreria(id).map_err(errore)?;
    con_libreria(&stato, |libreria| {
        incerti::correggi(&mut libreria.connection, id, &campi)?;
        incerti::uno(&libreria.connection, id)
    })
    .map_err(errore)
}

/// «La deduzione è giusta»: si tiene quel che c'è e non se ne parla più.
///
/// `(async)` per la stessa ragione di [`metadati_correggi`].
#[tauri::command(async)]
pub fn metadati_conferma(stato: State<'_, Stato>, id: i64) -> Esito<()> {
    // Un id di sessione non è una riga: vedi `riproduzione::ospiti`.
    let id = crate::riproduzione::riga_di_libreria(id).map_err(errore)?;
    con_libreria(&stato, |libreria| {
        incerti::conferma(&mut libreria.connection, id)
    })
    .map_err(errore)
}

/// «Rileggi tutti i tag»: la prossima scansione rilegge ogni file.
///
/// Non scansiona: segna. La scansione resta una sola, quella di sempre, con la
/// sua esclusione, il suo avanzamento e il suo Annulla — la finestra la avvia
/// subito dopo, e una seconda strada per leggere i file sarebbe una seconda
/// strada da tenere d'accordo con la prima. Il come sta su
/// `rymembra_app::library::rileggi_tutti_i_tag`; le correzioni a mano e
/// l'arricchimento sopravvivono alla rilettura come a ogni altra.
///
/// Restituisce quanti brani aspettano la rilettura.
///
/// In disparte: un `UPDATE` su ogni riga della libreria, che su centomila brani
/// non è una cosa da fare sul filo della finestra — e prima il turno.
///
/// # Il turno della scansione
///
/// Il segno si mette col turno di manutenzione in mano. Senza, una scansione
/// automatica a metà dei suoi lotti riscriveva la data vera sopra il segno di
/// ogni riga che salvava dopo, e quei brani saltavano in silenzio la
/// rilettura promessa.
///
/// # Errori
///
/// `library.scanBusy` se una scansione tiene il turno oltre l'attesa;
/// `db.queryFailed`.
#[tauri::command]
pub async fn metadati_rileggi_tutti(app: tauri::AppHandle) -> Esito<usize> {
    crate::disparte::in_disparte("rilettura dei tag", move || {
        let stato = crate::comandi::libreria_di(&app, "rilettura dei tag")?;
        rileggi_tutti_col_turno(&stato, ATTESA_DEL_TURNO)
    })
    .await
    .and_then(|esito| esito)
    .map_err(errore)
}

/// Quanto «Rileggi tutti i tag» aspetta che la scansione in corso lasci il
/// turno: come il trasloco.
const ATTESA_DEL_TURNO: std::time::Duration = std::time::Duration::from_secs(15);

/// Il corpo di [`metadati_rileggi_tutti`]: il turno, il segno, la riga nel
/// diario. La scansione che segue si scrive «a mano» come ogni altra: la riga
/// qui sopra è ciò che dice perché legge tutto.
fn rileggi_tutti_col_turno(
    stato: &Stato,
    entro: std::time::Duration,
) -> Result<usize, rymembra_domain::errors::AppError> {
    let _turno = crate::comandi::aspetta_il_turno_di_manutenzione(
        stato,
        entro,
        "la rilettura dei tag aspetta che la scansione in corso finisca",
    )?;
    let segnati = con_libreria(stato, |libreria| {
        rymembra_app::library::rileggi_tutti_i_tag(&libreria.connection)
    })?;
    crate::nota!("[scansione] rilettura dei tag chiesta: {segnati} brani segnati");
    Ok(segnati)
}

#[cfg(test)]
mod prove {
    use super::*;

    /// I brani con la data di modifica segnata per la rilettura.
    fn segnati(stato: &Stato) -> i64 {
        let guardia = stato.libreria.lock().expect("lucchetto");
        guardia
            .as_ref()
            .expect("libreria aperta")
            .connection
            .query_row(
                "SELECT COUNT(*) FROM tracks WHERE date_modified = -1",
                [],
                |riga| riga.get(0),
            )
            .expect("conto")
    }

    #[test]
    fn la_rilettura_senza_turno_non_segna_niente() {
        let dir = tempfile::tempdir().expect("cartella");
        let stato = Stato::apri(dir.path().to_path_buf());
        {
            let guardia = stato.libreria.lock().expect("lucchetto");
            guardia
                .as_ref()
                .expect("libreria aperta")
                .connection
                .execute(
                    "INSERT INTO tracks
                         (id, path, track_key, title, artist, album, duration_ms,
                          file_size, date_added, date_modified)
                     VALUES (1, 'D:/Musica/a.flac', 'k', 'T', 'A', 'Al', 1000, 9, 0, 5)",
                    [],
                )
                .expect("un brano");
        }

        // Una scansione ha il turno: la rilettura aspetta, poi rinuncia, e non
        // segna niente — un segno messo adesso lo riscriverebbe lei.
        let turno = stato.turno_di_manutenzione().expect("il turno");
        let rifiuto = rileggi_tutti_col_turno(&stato, std::time::Duration::from_millis(200))
            .expect_err("occupato");
        assert_eq!(rifiuto.code().kind().code(), "library.scanBusy");
        assert_eq!(segnati(&stato), 0);

        drop(turno);
        assert_eq!(
            rileggi_tutti_col_turno(&stato, std::time::Duration::from_millis(200)).expect("libero"),
            1
        );
        assert_eq!(segnati(&stato), 1);
    }
}
