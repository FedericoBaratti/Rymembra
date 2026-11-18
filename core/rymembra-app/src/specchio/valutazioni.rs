//! Mi piace e stelle sui brani della libreria unica.
//!
//! «Valgono per **tutte** le copie del gruppo»: un mi piace dato a una voce che
//! sta su più dispositivi vale per la copia di qui, che si scrive subito, e per
//! quelle degli altri dispositivi, che le possiede **loro**: la riga specchio si
//! aggiorna subito (così il cuore si accende), e la decisione va in posta
//! (`dispositivi_posta`, tipo `valuta`) per [`Domanda::Valuta`]: il
//! proprietario la scrive con la regola dell'orologio per campo
//! (`condivisa::valuta`) e la rimanda col suo catalogo.
//!
//! Finché la decisione aspetta in posta, il riconciliatore non riscrive quel
//! voto con quel che il proprietario aveva prima (`archivio::aggiorna`).
//!
//! [`Domanda::Valuta`]: https://docs.rs/ (vedi `rymembra-lontano`)

use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use rymembra_domain::errors::AppError;

use super::parti;
use crate::dispositivi::{self, POSTA_VALUTA};
use crate::library::db_error;

/// Il contenuto di una voce di posta `valuta`: il brano — con l'id del
/// proprietario — e quel che si è deciso. `None` per quel che non è cambiato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Valutazione {
    /// Il brano, con l'id nella libreria del proprietario.
    pub brano: i64,
    /// Il mi piace deciso, se è cambiato.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liked: Option<bool>,
    /// Le stelle decise, se sono cambiate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
}

impl Valutazione {
    /// Il contenuto di una voce di posta, o `None` se non si legge.
    #[must_use]
    pub fn da_corpo(corpo: &str) -> Option<Self> {
        serde_json::from_str(corpo).ok()
    }

    /// Il contenuto, in JSON.
    #[must_use]
    pub fn in_corpo(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Questa decisione con quella di prima che ancora aspettava: ognuno dei
    /// due campi vale l'ultimo che è stato deciso.
    #[must_use]
    pub fn dopo(self, prima: Option<Self>) -> Self {
        let Some(prima) = prima.filter(|p| p.brano == self.brano) else {
            return self;
        };
        Self {
            brano: self.brano,
            liked: self.liked.or(prima.liked),
            rating: self.rating.or(prima.rating),
        }
    }
}

/// Un mi piace o un voto dato a `id`: le altre copie del suo gruppo lo
/// seguono, e per le righe specchio — compresa `id`, se lo è — la decisione va
/// in posta. Restituisce i dispositivi con posta nuova, da svegliare.
///
/// `id` **lo scrive già chi chiama**, col suo comando di sempre: qui si
/// scrivono le altre, e la posta. Un brano senza gruppo e che non è una riga
/// specchio non fa niente.
///
/// # Errori
///
/// Quelli del database.
pub fn dai_al_gruppo(
    connection: &Connection,
    id: i64,
    liked: Option<bool>,
    rating: Option<i64>,
    adesso_ms: i64,
) -> Result<Vec<String>, AppError> {
    let cosa = "valutazione di un gruppo di copie";
    if liked.is_none() && rating.is_none() {
        return Ok(Vec::new());
    }
    let Some(copia_di) = connection
        .query_row("SELECT copia_di FROM tracks WHERE id = ?1", [id], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .ok()
    else {
        return Ok(Vec::new());
    };
    let tx: Transaction<'_> =
        Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
            .map_err(|err| db_error(cosa, &err))?;
    let radice = copia_di.unwrap_or(id);
    let membri: Vec<(i64, String)> = {
        let mut st = tx
            .prepare(
                "SELECT id, path FROM tracks WHERE (id = ?1 OR copia_di = ?1) AND path IS NOT NULL",
            )
            .map_err(|err| db_error(cosa, &err))?;
        let lette = st
            .query_map([radice], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|err| db_error(cosa, &err))?;
        lette
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    let mut svegliati: Vec<String> = Vec::new();
    for (membro, path) in membri {
        if membro != id {
            if let Some(liked) = liked {
                tx.execute(
                    "UPDATE tracks SET liked = ?2, liked_at = ?3, stats_updated_at = ?3 WHERE id = ?1",
                    params![membro, i64::from(liked), adesso_ms],
                )
                .map_err(|err| db_error(cosa, &err))?;
            }
            if let Some(stelle) = rating {
                tx.execute(
                    "UPDATE tracks SET rating = ?2, rating_at = ?3, stats_updated_at = ?3 WHERE id = ?1",
                    params![membro, stelle.clamp(0, 5), adesso_ms],
                )
                .map_err(|err| db_error(cosa, &err))?;
            }
        }
        let Some((dispositivo, remoto)) = parti(&path) else {
            continue;
        };
        let chiave = remoto.to_string();
        let prima = dispositivi::in_posta(&tx, dispositivo, POSTA_VALUTA, &chiave)?
            .and_then(|voce| Valutazione::da_corpo(&voce.corpo));
        let corpo = Valutazione {
            brano: remoto,
            liked,
            rating: rating.map(|s| s.clamp(0, 5)),
        }
        .dopo(prima)
        .in_corpo();
        dispositivi::metti_in_posta(&tx, dispositivo, POSTA_VALUTA, &chiave, &corpo, adesso_ms)?;
        if !svegliati.iter().any(|d| d == dispositivo) {
            svegliati.push(dispositivo.to_owned());
        }
    }
    tx.commit().map_err(|err| db_error(cosa, &err))?;
    Ok(svegliati)
}

#[cfg(test)]
mod prove {
    use super::*;
    use crate::specchio::comuni::{ADESSO, chiave, libreria};

    fn locale(c: &Connection, id: i64, copia_di: Option<i64>) {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added,
                                 date_modified, copia_di)
             VALUES (?1, ?2, 'k', 'T', 'A', 'B', 1, 1, 1, ?3)",
            params![id, format!("C:/M/{id}.mp3"), copia_di],
        )
        .unwrap();
    }

    fn remota(c: &Connection, id: i64, dispositivo: &str, id_remoto: i64, copia_di: Option<i64>) {
        c.execute(
            "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added,
                                 date_modified, source, copia_di)
             VALUES (?1, ?2, 'k', 'T', 'A', 'B', 1, 1, 1, 'lontano', ?3)",
            params![
                id,
                crate::specchio::percorso(dispositivo, id_remoto),
                copia_di
            ],
        )
        .unwrap();
        c.execute(
            "INSERT INTO lontano_brani (track_id, dispositivo, id_remoto) VALUES (?1, ?2, ?3)",
            params![id, dispositivo, id_remoto],
        )
        .unwrap();
    }

    fn stato(c: &Connection, id: i64) -> (i64, Option<i64>, i64, Option<i64>) {
        c.query_row(
            "SELECT liked, liked_at, rating, rating_at FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    #[test]
    fn un_brano_senza_gruppo_e_locale_non_fa_niente() {
        let c = libreria();
        locale(&c, 1, None);
        assert!(
            dai_al_gruppo(&c, 1, Some(true), None, ADESSO)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM dispositivi_posta", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        // Un brano che non c'è: niente.
        assert!(
            dai_al_gruppo(&c, 99, Some(true), None, ADESSO)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn un_mi_piace_alla_voce_di_un_gruppo_vale_per_tutte_le_copie_e_va_al_proprietario() {
        // La voce mostrata è la copia del PC (id 2); la locale (id 1) sta
        // dietro di lei. Un mi piace dato alla voce: la locale lo prende
        // subito, la remota lo ha già dal comando, e al PC va una valutazione.
        let c = libreria();
        let pc = chiave('a');
        locale(&c, 1, Some(2));
        remota(&c, 2, &pc, 77, None);
        // Il comando ha già scritto la riga `id` (la remota, ottimisticamente).
        c.execute(
            "UPDATE tracks SET liked = 1, liked_at = ?1, stats_updated_at = ?1 WHERE id = 2",
            [ADESSO],
        )
        .unwrap();
        let da_svegliare = dai_al_gruppo(&c, 2, Some(true), None, ADESSO).unwrap();
        assert_eq!(da_svegliare, vec![pc.clone()]);
        assert_eq!(stato(&c, 1).0, 1, "la copia di qui, subito");
        assert_eq!(stato(&c, 1).1, Some(ADESSO));
        let voce = dispositivi::in_posta(&c, &pc, POSTA_VALUTA, "77")
            .unwrap()
            .expect("in posta");
        assert_eq!(voce.quando_ms, ADESSO);
        assert_eq!(
            Valutazione::da_corpo(&voce.corpo),
            Some(Valutazione {
                brano: 77,
                liked: Some(true),
                rating: None
            })
        );
    }

    #[test]
    fn dato_alla_copia_di_qui_arriva_anche_alla_remota_del_gruppo() {
        // Il caso inverso: si dà il voto alla copia locale (id 1, nascosta);
        // la remota (id 2) è la voce del gruppo.
        let c = libreria();
        let pc = chiave('b');
        locale(&c, 1, Some(2));
        remota(&c, 2, &pc, 5, None);
        let svegliati = dai_al_gruppo(&c, 1, None, Some(4), ADESSO).unwrap();
        assert_eq!(svegliati, vec![pc.clone()]);
        let (_, _, voto, quando) = stato(&c, 2);
        assert_eq!(
            (voto, quando),
            (4, Some(ADESSO)),
            "la riga specchio, subito"
        );
        let voce = dispositivi::in_posta(&c, &pc, POSTA_VALUTA, "5")
            .unwrap()
            .unwrap();
        assert_eq!(
            Valutazione::da_corpo(&voce.corpo),
            Some(Valutazione {
                brano: 5,
                liked: None,
                rating: Some(4)
            })
        );
    }

    #[test]
    fn due_decisioni_sullo_stesso_brano_si_uniscono_in_una_voce_sola() {
        let c = libreria();
        let pc = chiave('c');
        remota(&c, 2, &pc, 5, None);
        dai_al_gruppo(&c, 2, Some(true), None, ADESSO).unwrap();
        dai_al_gruppo(&c, 2, None, Some(3), ADESSO + 10).unwrap();
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM dispositivi_posta", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let voce = dispositivi::in_posta(&c, &pc, POSTA_VALUTA, "5")
            .unwrap()
            .unwrap();
        assert_eq!(voce.quando_ms, ADESSO + 10, "la più nuova");
        assert_eq!(
            Valutazione::da_corpo(&voce.corpo),
            Some(Valutazione {
                brano: 5,
                liked: Some(true),
                rating: Some(3)
            })
        );
        // E un mi piace tolto dopo vince sul mi piace di prima.
        dai_al_gruppo(&c, 2, Some(false), None, ADESSO + 20).unwrap();
        let voce = dispositivi::in_posta(&c, &pc, POSTA_VALUTA, "5")
            .unwrap()
            .unwrap();
        assert_eq!(
            Valutazione::da_corpo(&voce.corpo),
            Some(Valutazione {
                brano: 5,
                liked: Some(false),
                rating: Some(3)
            })
        );
    }

    #[test]
    fn due_copie_di_due_dispositivi_scrivono_a_tutti_e_due() {
        let c = libreria();
        let (a, b) = (chiave('a'), chiave('b'));
        remota(&c, 1, &a, 10, None);
        remota(&c, 2, &b, 20, Some(1));
        let mut svegliati = dai_al_gruppo(&c, 1, Some(true), None, ADESSO).unwrap();
        svegliati.sort();
        assert_eq!(svegliati, vec![a.clone(), b.clone()]);
        assert!(
            dispositivi::in_posta(&c, &a, POSTA_VALUTA, "10")
                .unwrap()
                .is_some()
        );
        assert!(
            dispositivi::in_posta(&c, &b, POSTA_VALUTA, "20")
                .unwrap()
                .is_some()
        );
        assert_eq!(stato(&c, 2).0, 1);
    }

    #[test]
    fn la_posta_si_consegna_una_volta_e_una_scrittura_piu_nuova_resta() {
        let c = libreria();
        let pc = chiave('d');
        remota(&c, 2, &pc, 5, None);
        dai_al_gruppo(&c, 2, Some(true), None, ADESSO).unwrap();
        let mandata = dispositivi::posta_da_mandare(&c, &pc).unwrap();
        assert_eq!(mandata.len(), 1);
        // Mentre la voce partiva, una decisione più nuova per lo stesso brano.
        dai_al_gruppo(&c, 2, Some(false), None, ADESSO + 5).unwrap();
        assert!(!dispositivi::posta_consegnata(&c, mandata[0].id, mandata[0].quando_ms).unwrap());
        assert_eq!(
            dispositivi::posta_da_mandare(&c, &pc).unwrap().len(),
            1,
            "quella nuova resta"
        );
        // Consegnata anche quella.
        let nuova = dispositivi::posta_da_mandare(&c, &pc).unwrap();
        assert!(dispositivi::posta_consegnata(&c, nuova[0].id, nuova[0].quando_ms).unwrap());
        assert!(dispositivi::posta_da_mandare(&c, &pc).unwrap().is_empty());
        assert!(dispositivi::con_posta_in_attesa(&c).unwrap().is_empty());
    }

    #[test]
    fn dimenticare_un_dispositivo_butta_la_sua_posta() {
        let c = libreria();
        let pc = chiave('e');
        c.execute(
            "INSERT INTO dispositivi (chiave, nome, associato_ms) VALUES (?1, 'PC', 1)",
            [&pc],
        )
        .unwrap();
        remota(&c, 2, &pc, 5, None);
        dai_al_gruppo(&c, 2, Some(true), None, ADESSO).unwrap();
        assert!(dispositivi::dimentica(&c, &pc).is_ok());
        assert!(dispositivi::posta_da_mandare(&c, &pc).unwrap().is_empty());
    }
}
