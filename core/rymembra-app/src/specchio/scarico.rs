//! «Scarica qui» partendo dalle righe della libreria: da righe di `tracks` ai
//! brani da chiedere al loro proprietario.
//!
//! Prima si scaricava dalla pagina «Sfoglia», che aveva in mano le schede dei
//! brani dell'altro (`BranoRemoto`). Con la libreria unica la finestra ha
//! soltanto gli id delle righe, e la scheda si ricostruisce qui da quel che il
//! catalogo ha già scritto: il titolo, i numeri, la dimensione e la versione
//! del file, l'estensione e l'id **nella libreria dell'altro**.

use rusqlite::{Connection, OptionalExtension as _};
use rymembra_domain::errors::AppError;

use crate::library::condivisa::{BranoRemoto, Segmento};
use crate::library::db_error;

/// I brani di `righe` da scaricare, raggruppati per dispositivo e nell'ordine
/// in cui compaiono.
///
/// Escono solo le righe che si possono davvero chiedere: una riga di questa
/// macchina non c'è da scaricare, e una che il proprietario non manda più
/// (`sparito_ms`) non ha più un file da dare. Si saltano senza errore: la
/// finestra offre il gesto su una selezione, e una selezione può essere mista.
///
/// # Errori
///
/// `db.queryFailed` se il database non risponde.
pub fn righe_da_scaricare(
    connection: &Connection,
    righe: &[i64],
) -> Result<Vec<(String, Vec<BranoRemoto>)>, AppError> {
    let cosa = "righe da scaricare";
    let mut statement = connection
        .prepare_cached(
            "SELECT l.dispositivo, l.id_remoto, t.title, t.artist, t.album, t.track_number,
                    t.disc_number, t.duration_ms, t.year, l.estensione, t.file_size,
                    l.versione, l.inizio_cd, l.fine_cd
               FROM tracks t JOIN lontano_brani l ON l.track_id = t.id
              WHERE t.id = ?1 AND l.sparito_ms IS NULL",
        )
        .map_err(|err| db_error(cosa, &err))?;
    let mut gruppi: Vec<(String, Vec<BranoRemoto>)> = Vec::new();
    for id in righe {
        let trovato = statement
            .query_row([id], |r| {
                let inizio_cd: Option<i64> = r.get(12)?;
                let fine_cd: Option<i64> = r.get(13)?;
                Ok((
                    r.get::<_, String>(0)?,
                    BranoRemoto {
                        id: r.get(1)?,
                        title: r.get(2)?,
                        artist: r.get(3)?,
                        album: r.get(4)?,
                        album_key: None,
                        track_number: r.get(5)?,
                        disc_number: r.get(6)?,
                        duration_ms: r.get(7)?,
                        year: r.get(8)?,
                        cover_art_hash: None,
                        play_count: 0,
                        liked: false,
                        rating: 0,
                        estensione: r.get(9)?,
                        dimensione: r.get(10)?,
                        versione: r.get(11)?,
                        segmento: inizio_cd.map(|inizio_cd| Segmento { inizio_cd, fine_cd }),
                    },
                ))
            })
            .optional()
            .map_err(|err| db_error(cosa, &err))?;
        let Some((dispositivo, brano)) = trovato else {
            continue;
        };
        match gruppi.iter_mut().find(|(d, _)| *d == dispositivo) {
            Some((_, brani)) => brani.push(brano),
            None => gruppi.push((dispositivo, vec![brano])),
        }
    }
    Ok(gruppi)
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::specchio::comuni::{ADESSO, chiave, giro, libreria, riga};

    fn righe_di(c: &Connection) -> Vec<i64> {
        c.prepare("SELECT id FROM tracks WHERE path LIKE 'lontano://%' ORDER BY id")
            .and_then(|mut q| q.query_map([], |r| r.get(0)).and_then(Iterator::collect))
            .expect("righe specchio")
    }

    #[test]
    fn le_righe_diventano_i_brani_da_chiedere_al_proprietario() {
        let c = libreria();
        let (pc, portatile) = (chiave('a'), chiave('b'));
        let mut con_cd = riga(3);
        con_cd.segmento = Some(Segmento {
            inizio_cd: 750,
            fine_cd: Some(900),
        });
        giro(&c, &pc, &[riga(1), riga(2), con_cd], ADESSO);
        giro(&c, &portatile, &[riga(4)], ADESSO);
        // Una riga di questa macchina, nella stessa selezione.
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                 date_added, date_modified)
             VALUES (900, '/m/uno.mp3', 'k', 'Uno', 'A', 'B', 1, 1, 1)",
            [],
        )
        .expect("brano di qui");
        let ids = righe_di(&c);
        assert_eq!(ids.len(), 4);
        let mut scelte = ids.clone();
        scelte.push(900);

        let gruppi = righe_da_scaricare(&c, &scelte).expect("righe");
        assert_eq!(gruppi.len(), 2, "uno per dispositivo: {gruppi:?}");
        let (primo, brani_del_pc) = &gruppi[0];
        assert_eq!(primo, &pc, "nell'ordine in cui compaiono");
        assert_eq!(
            brani_del_pc.iter().map(|b| b.id).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "l'id è quello nella libreria dell'altro"
        );
        let uno = &brani_del_pc[0];
        assert_eq!(uno.title, "Titolo 1");
        assert_eq!(uno.estensione, "mp3");
        assert_eq!(uno.dimensione, 8_000_000);
        assert_eq!(uno.versione, "5000000-1700000000000");
        assert!(uno.segmento.is_none());
        assert_eq!(
            brani_del_pc[2].segmento,
            Some(Segmento {
                inizio_cd: 750,
                fine_cd: Some(900)
            }),
            "un brano d'immagine porta il suo pezzo: lo scaricamento lo salta"
        );
        assert_eq!(gruppi[1].0, portatile);
        assert_eq!(gruppi[1].1.len(), 1);
        assert!(
            gruppi.iter().flat_map(|(_, b)| b).all(|b| b.id != 900),
            "una riga di qui non è da scaricare"
        );
    }

    #[test]
    fn una_riga_che_il_proprietario_non_manda_piu_non_si_chiede() {
        let c = libreria();
        let pc = chiave('a');
        giro(&c, &pc, &[riga(1), riga(2)], ADESSO);
        let ids = righe_di(&c);
        c.execute(
            "UPDATE lontano_brani SET sparito_ms = 1 WHERE track_id = ?1",
            [ids[0]],
        )
        .expect("sparita");
        let gruppi = righe_da_scaricare(&c, &ids).expect("righe");
        let rimasti: Vec<i64> = gruppi
            .iter()
            .flat_map(|(_, b)| b.iter().map(|b| b.id))
            .collect();
        assert_eq!(rimasti, vec![2], "solo quella che c'è ancora");
        assert!(righe_da_scaricare(&c, &[]).expect("niente").is_empty());
        assert!(
            righe_da_scaricare(&c, &[123_456])
                .expect("una riga che non c'è")
                .is_empty()
        );
    }
}
