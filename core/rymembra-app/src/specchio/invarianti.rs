//! Le invarianti di §4.7: dove una riga `lontano://` NON deve entrare.
//!
//! Il catalogo di un altro dispositivo è musica di un altro dispositivo. Una
//! riga specchio ha un `id` e sta in `tracks`, e quindi qualunque interrogazione
//! scritta prima di lei — che dà per scontato che «una riga di `tracks` è un
//! file di questo disco» — la prende per buona. Questo è l'elenco dei posti che
//! trattano `nuvola://` (che hanno la stessa forma del problema), la decisione
//! presa per ciascuno, e la prova che la tiene vera.
//!
//! | Dove | Decisione | Prova |
//! |---|---|---|
//! | `library/condivisa.rs` (`CONDIVISO`) | mai ricondivise: un terzo dispositivo non vede i brani del primo attraverso il secondo; e solo i brani che stanno **adesso** sotto una cartella sorvegliata (una cartella tolta esce subito, anche dalle cose ricordate) | `library::prove_catalogo` |
//! | `sincronia.rs`, ascolti e statistiche | le statistiche di una riga specchio sono una copia di quelle del proprietario: non entrano nei contatori, non si sommano, non partono, non si riscrivono da fuori | `le_statistiche_di_una_riga_specchio_non_entrano_nella_sincronia`, `quel_che_arriva_dalla_sincronia_non_scrive_sulle_righe_specchio` |
//! | `sincronia.rs`, playlist (FSYNC) | due regole: **per agganciare** una riga specchio conta come presente (una voce viva entra in `playlist_tracks` anche su una riga del PC); **per seppellire** una chiave presente qui solo attraverso righe specchio conta come assente (invisibile, non tolta). Una riga che nasce, cambia chiave o sparisce non è mai un gesto: la lapide la scrive solo il gesto dell'utente, esplicita | gli scenari `fsync_*` (l'ultimo in `library::tests`), `specchio::gesti` e `specchio::revisione` |
//! | `backup.rs`, `profilo/biblioteca.rs` | né brani né cronologia né copertine né cartelle: sono fatti di questa macchina, come `nuvola_voci` | `backup_e_profilo_non_portano_le_righe_specchio` |
//! | `library/righe.rs`, `library/scansione.rs` | la scansione non le conosce: non le toglie, non le riscrive, non le trova doppie (`RemoveReason::DuplicateRow`), «Rileggi tutti i tag» non le segna | `library::tests::una_scansione_non_toglie_ne_riscrive_le_righe_specchio` e due sorelle |
//! | `library/righe.rs`, `cancella_brani` | non si tolgono a mano: tornerebbero al giro dopo | `le_righe_specchio_non_si_tolgono_a_mano` |
//! | `files.rs`, `scrittura.rs`, `tag_scrittura.rs` | non hanno un file qui: né si aprono da disco né si scrivono (`lontano.nonQui`) | `una_riga_specchio_non_e_un_file` |
//! | `volumi.rs` | «di rete» e «ignota»: niente Cestino, niente proposta di rimappare | `una_riga_specchio_non_e_un_file` |
//! | `cartelle.rs`, `profilo/radici.rs` | non compaiono nell'albero e non diventano mai una radice | `le_cartelle_non_vedono_le_righe_specchio`, `profilo::radici::prove` |
//! | `import_playlist.rs` | fuori da un file di playlist, come i brani del cloud | `una_riga_specchio_non_e_un_file` |
//! | `enrich.rs`, `intensita.rs`, `sonora.rs`, `nuvola.rs` | mai candidate: arricchimento, volume, impronta leggono il file | `le_passate_sui_file_non_le_prendono` |
//! | `enrich.rs`, il gruppo d'album | mai nel gruppo: il disco candidato si rilegge per `album_key`, e le righe specchio hanno la chiave di qui; non si interrogano, non si riscrivono, non si annotano | `il_gruppo_d_arricchimento_non_porta_le_righe_specchio`, `l_arricchimento_non_riscrive_una_riga_specchio`, `dimentica_l_arricchimento_non_conta_le_righe_specchio` |
//! | `scrobble.rs` | mai scrobblate da qui: lo scrobble lo fa il proprietario | `una_riga_specchio_non_si_scrobbla_da_qui` |
//! | `cancellazione.rs`, trasloco, sorveglianza, analisi | agiscono su file veri e percorsi veri: la guardia sta sotto (`LocalFiles`, `sola_lettura`, `known_tracks`) | le prove di sopra |
//! | Ascolta insieme | il cancello di oggi resta, controllato sullo schema del percorso | `riproduzione::insieme::prove::una_riga_della_libreria_unica_non_entra_in_una_sessione` |
//! | ricerca | le trova già: i trigger `tracks_fts_*` le indicizzano | `library::prove_catalogo` |

#[cfg(test)]
mod prove {
    use std::collections::BTreeMap;
    use std::path::Path;

    use rusqlite::{Connection, params};
    use rymembra_domain::enrich::{Fields, Verdetto};
    use rymembra_domain::errors::ErrorCodeKind;
    use rymembra_domain::paths::PathRules;

    use crate::specchio::archivio::{GRAZIA_MS, dimentica_dispositivo};
    use crate::specchio::comuni::{ADESSO, chiave, giro, libreria, riga};
    use crate::{backup, sincronia};

    /// Il dispositivo di prova: il PC dell'altro lato.
    const PC: char = 'a';

    fn n(c: &Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).expect("conteggio")
    }

    /// Un brano di questa macchina, con la chiave dei brani di [`riga`].
    fn brano_di_qui(c: &Connection, id: i64, numero: i64) {
        let chiave_del_brano = riga(numero).track_key;
        c.execute(
            "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album, file_size,
                             date_added, date_modified)
         VALUES (?1, ?2, ?3, ?3, ?4, 'Artista', 'Album', 1000, 1, 1)",
            params![
                id,
                format!("/musica/{numero}.mp3"),
                chiave_del_brano,
                format!("Titolo {numero}")
            ],
        )
        .expect("brano di qui");
    }

    /// L'id della riga specchio del brano `numero` del PC.
    fn id_specchio(c: &Connection, numero: i64) -> i64 {
        c.query_row(
            "SELECT track_id FROM lontano_brani WHERE dispositivo = ?1 AND id_remoto = ?2",
            params![chiave(PC), numero],
            |r| r.get(0),
        )
        .expect("riga specchio")
    }

    fn playlist_con(c: &Connection, nome: &str, brani: &[i64]) {
        c.execute(
            "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
         VALUES (?1, ?1, 100, 100)",
            [nome],
        )
        .expect("playlist");
        let id: i64 = c
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = ?1",
                [nome],
                |r| r.get(0),
            )
            .expect("identificativo");
        for (posizione, brano) in brani.iter().enumerate() {
            c.execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
                params![id, brano, i64::try_from(posizione).expect("posizione")],
            )
            .expect("membro");
        }
    }

    /// I brani di una playlist, nell'ordine, per chiave, come sono in `playlist_tracks`.
    fn in_playlist(c: &Connection, nome: &str) -> Vec<String> {
        let mut st = c
            .prepare(
                "SELECT t.track_key FROM playlist_tracks pt
               JOIN playlists p ON p.id = pt.playlist_id
               JOIN tracks t ON t.id = pt.track_id
              WHERE p.playlist_key = ?1 ORDER BY pt.position",
            )
            .expect("ordine");
        st.query_map([nome], |r| r.get::<_, String>(0))
            .expect("ordine")
            .map(|r| r.expect("riga"))
            .collect()
    }

    /// La playlist come la pubblica la sincronia: le voci vive nell'ordine, e quante
    /// voci tolte (lapidi) porta.
    fn pubblicata(c: &Connection, nome: &str) -> (Vec<String>, usize) {
        let contenuto =
            sincronia::contenuto(c, "qui", BTreeMap::new(), BTreeMap::new()).expect("contenuto");
        let sequenza = &contenuto
            .playlist
            .get(nome)
            .expect("la playlist viaggia")
            .sequenza;
        let vive = sequenza.ordine().into_iter().map(str::to_owned).collect();
        let tolte = sequenza
            .elementi()
            .filter(|(_, e)| e.tolto.is_some())
            .count();
        (vive, tolte)
    }

    // ── FSYNC ───────────────────────────────────────────────────────────────────

    #[test]
    fn fsync_una_playlist_con_un_brano_solo_remoto_viaggia_intera() {
        // Il telefono ha il brano 1 e vede il brano 2 solo come riga specchio. La
        // playlist li ha tutti e due: la sincronia la pubblica con tutti e due, per
        // chiave, e due passate di fila non cambiano niente.
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let remoto = id_specchio(&c, 2);
        playlist_con(&c, "sera", &[1, remoto]);

        sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
        let (vive, tolte) = pubblicata(&c, "sera");
        assert_eq!(vive, vec![riga(1).track_key, riga(2).track_key]);
        assert_eq!(tolte, 0);
        // La riga resta in playlist: non la si toglie «perché non è un file».
        assert_eq!(
            in_playlist(&c, "sera"),
            vec![riga(1).track_key, riga(2).track_key]
        );
        let ancora = sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
        assert!(ancora.e_vuoto(), "{ancora:?}");
        // Una playlist con un brano remoto porta solo la sua chiave: né il
        // percorso né il dispositivo da cui viene.
        let json = serde_json::to_string(
            &sincronia::contenuto(&c, "telefono", BTreeMap::new(), BTreeMap::new())
                .expect("contenuto"),
        )
        .expect("in JSON");
        assert!(!json.contains("lontano://"), "{json}");
        assert!(!json.contains(&chiave(PC)), "{json}");
    }

    #[test]
    fn fsync_un_dispositivo_dimenticato_non_toglie_i_suoi_brani_dalle_playlist_degli_altri() {
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        playlist_con(&c, "sera", &[1, id_specchio(&c, 2)]);
        sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
        let prima = pubblicata(&c, "sera");

        // «Dimentica questo dispositivo»: la riga sparisce, e con lei la sua voce
        // di playlist per cascata.
        let (rimappate, tolte) = dimentica_dispositivo(&c, &chiave(PC)).expect("dimenticato");
        assert_eq!((rimappate, tolte), (0, 1));
        assert_eq!(in_playlist(&c, "sera"), vec![riga(1).track_key]);

        // Per la sincronia non è l'utente che ha tolto il brano dalla playlist: la
        // voce resta viva, e niente lapide parte per gli altri dispositivi.
        sincronia::allinea(&mut c, "telefono", 3_000).expect("allineamento");
        assert_eq!(pubblicata(&c, "sera"), prima);

        // E se il PC torna, la voce torna al suo posto da sola.
        giro(&c, &chiave(PC), &[riga(2)], ADESSO + 10_000);
        let torna = sincronia::allinea(&mut c, "telefono", 4_000).expect("allineamento");
        assert_eq!(torna.agganciate, 1, "{torna:?}");
        assert_eq!(
            in_playlist(&c, "sera"),
            vec![riga(1).track_key, riga(2).track_key]
        );
        assert_eq!(pubblicata(&c, "sera"), prima);
    }

    #[test]
    fn fsync_un_brano_tolto_sul_pc_non_e_una_decisione_di_questo_telefono() {
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2), riga(3)], ADESSO);
        playlist_con(&c, "sera", &[1, id_specchio(&c, 2), id_specchio(&c, 3)]);
        sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
        let prima = pubblicata(&c, "sera");
        assert_eq!(prima.0.len(), 3);

        // Il PC toglie il brano 2: l'altro giro non lo manda più, la riga resta
        // nascosta per la grazia, e passata quella (pochi brani) se ne va, come
        // per una scansione.
        giro(&c, &chiave(PC), &[riga(3)], ADESSO + 10_000);
        let chiusura = giro(
            &c,
            &chiave(PC),
            &[riga(3)],
            ADESSO + 10_000 + GRAZIA_MS + 1_000,
        );
        assert_eq!(chiusura.tolte, 1, "{chiusura:?}");
        assert_eq!(
            in_playlist(&c, "sera"),
            vec![riga(1).track_key, riga(3).track_key]
        );

        sincronia::allinea(&mut c, "telefono", 3_000).expect("allineamento");
        assert_eq!(
            pubblicata(&c, "sera"),
            prima,
            "nessuna voce tolta: il brano manca, non è stato tolto dalla playlist"
        );
    }

    #[test]
    fn fsync_il_brano_che_torna_prima_del_prossimo_allineamento_non_e_una_rimozione() {
        // La finestra stretta: la riga se ne va e ritorna fra due allineamenti, e
        // nessuno ha avuto il tempo di ricordare che il brano mancava. Senza la
        // memoria scritta alla rimozione, il brano tornato avrebbe una voce viva
        // in sequenza e nessuna riga in playlist: identico a un brano che l'utente
        // ha appena tolto, e la lapide partirebbe per tutti.
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2), riga(3)], ADESSO);
        playlist_con(&c, "sera", &[1, id_specchio(&c, 2), id_specchio(&c, 3)]);
        sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
        let prima = pubblicata(&c, "sera");

        giro(&c, &chiave(PC), &[riga(3)], ADESSO + 10_000);
        giro(
            &c,
            &chiave(PC),
            &[riga(3)],
            ADESSO + 10_000 + GRAZIA_MS + 1_000,
        );
        assert_eq!(in_playlist(&c, "sera").len(), 2);
        giro(
            &c,
            &chiave(PC),
            &[riga(2), riga(3)],
            ADESSO + 20_000 + GRAZIA_MS,
        );

        let fatto = sincronia::allinea(&mut c, "telefono", 3_000).expect("allineamento");
        assert_eq!(fatto.agganciate, 1, "{fatto:?}");
        assert_eq!(
            in_playlist(&c, "sera"),
            vec![riga(1).track_key, riga(2).track_key, riga(3).track_key]
        );
        assert_eq!(pubblicata(&c, "sera"), prima, "nessuna voce tolta");
    }

    #[test]
    fn fsync_limite_rovesciato_una_correzione_di_chiave_con_una_copia_specchio() {
        // Era il limite dichiarato di F2 («una riga specchio è un membro di
        // playlist, ma non conta come avere il file»): qui si correggeva un tag e
        // la lapide partiva verso il PC. La revisione ha mostrato che era una
        // perdita di dati su un altro dispositivo, e la prova adesso asserisce che
        // non si perde niente.
        //
        // Il brano sta qui (riga 1) e sul PC (riga specchio) con la stessa chiave.
        // Qui si corregge un tag e la chiave della riga di qui cambia. Il PC ha
        // ancora la vecchia chiave, quindi per questa libreria la vecchia chiave
        // «c'è ancora» solo come riga specchio: per la lapide un brano che c'è
        // solo così conta come assente (`sincronia::brani_assenti`), la voce viva
        // resta com'è, e la lapide non parte. Quel che toglie una voce così è il
        // gesto dell'utente, che la scrive da sé (`specchio::gesti`).
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(1)], ADESSO);
        playlist_con(&c, "sera", &[1]);
        sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
        assert_eq!(pubblicata(&c, "sera"), (vec![riga(1).track_key], 0));

        c.execute(
            "UPDATE tracks SET track_key = 'corretta|uno|album' WHERE id = 1",
            [],
        )
        .expect("correzione");
        sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
        let (vive, tolte) = pubblicata(&c, "sera");
        assert_eq!(
            tolte, 0,
            "nessuna lapide: la vecchia voce non è stata tolta"
        );
        assert!(
            vive.contains(&riga(1).track_key) && vive.contains(&"corretta|uno|album".to_owned()),
            "la vecchia voce resta viva e la nuova la affianca: {vive:?}"
        );
    }

    // ── la sincronia: le statistiche ────────────────────────────────────────────

    #[test]
    fn le_statistiche_di_una_riga_specchio_non_entrano_nella_sincronia() {
        // Una riga specchio porta una copia delle statistiche del proprietario:
        // sette ascolti, quattro stelle, il cuoricino. Se la sincronia le contasse
        // come fatti di questa macchina, il telefono li pubblicherebbe come suoi, il
        // PC li sommerebbe ai propri, e al giro dopo il numero crescerebbe da solo.
        let mut c = libreria();
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let remoto = id_specchio(&c, 2);
        c.execute(
            "UPDATE tracks SET play_count = 7, rating = 4, rating_at = 500, liked = 1,
                           liked_at = 600, last_played_at = 700, stats_updated_at = 700
          WHERE id = ?1",
            [remoto],
        )
        .expect("statistiche copiate");

        let fatto = sincronia::allinea(&mut c, "telefono", 1_000).expect("allineamento");
        assert_eq!(fatto.ascolti, 0, "{fatto:?}");
        assert_eq!(n(&c, "SELECT COUNT(*) FROM sync_ascolti"), 0);
        let copiati = "SELECT play_count FROM tracks WHERE path LIKE 'lontano://%'";
        assert_eq!(
            n(&c, copiati),
            7,
            "la copia resta com'è: la riscrive solo il giro dello specchio"
        );
        let contenuto = sincronia::contenuto(&c, "telefono", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let k = riga(2).track_key;
        assert!(!contenuto.ascolti.contains_key(&k));
        assert!(!contenuto.ereditati.contains_key(&k));
        assert!(!contenuto.ultimo.contains_key(&k));
        assert!(!contenuto.voti.contains_key(&k));
        assert!(!contenuto.preferiti.contains_key(&k));

        // Un ascolto vero, di una copia di qui con la stessa chiave, si conta una
        // volta sola e non tocca la copia.
        brano_di_qui(&c, 9, 2);
        let tx = crate::library::transazione_di_scrittura(&mut c).expect("transazione");
        sincronia::conta_ascolto(&tx, 9, "telefono").expect("conteggio");
        tx.commit().expect("chiusura");
        assert_eq!(
            n(&c, "SELECT COALESCE(SUM(quanti), 0) FROM sync_ascolti"),
            1
        );
        assert_eq!(n(&c, "SELECT play_count FROM tracks WHERE id = 9"), 1);
        assert_eq!(n(&c, copiati), 7);
        sincronia::allinea(&mut c, "telefono", 2_000).expect("allineamento");
        assert_eq!(n(&c, "SELECT play_count FROM tracks WHERE id = 9"), 1);
        assert_eq!(n(&c, copiati), 7);
    }

    #[test]
    fn quel_che_arriva_dalla_sincronia_non_scrive_sulle_righe_specchio() {
        use rymembra_sync::{Contenuto, Voto};
        use rymembra_sync::{Documento, fondi};
        // Un altro dispositivo dà quattro stelle al brano 2, e noi ne abbiamo solo
        // la riga specchio: non è una riga che la sincronia possa toccare.
        let mut c = libreria();
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let mut suo = Contenuto::default();
        suo.voti.insert(riga(2).track_key, Voto { v: 4, at: 5_000 });
        let fuso = fondi(&[Documento::nuovo("altro", suo, 6_000)]);
        sincronia::applica(&mut c, &fuso, 6_000).expect("applicazione");
        assert_eq!(
            n(
                &c,
                "SELECT rating FROM tracks WHERE path LIKE 'lontano://%'"
            ),
            0
        );
    }

    // ── backup e profilo ────────────────────────────────────────────────────────

    #[test]
    fn backup_e_profilo_non_portano_le_righe_specchio() {
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let remoto = id_specchio(&c, 2);
        c.execute(
        "UPDATE tracks SET play_count = 7, rating = 4, rating_at = 500, liked = 1, liked_at = 600,
                           last_played_at = 700, stats_updated_at = 700
          WHERE id = ?1",
        [remoto],
    )
    .expect("statistiche copiate");
        c.execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, 1700, 90000)",
            [remoto],
        )
        .expect("ascolto di una riga lontana");
        playlist_con(&c, "sera", &[1, remoto]);
        crate::settings::write_json(
            &c,
            crate::settings::CHIAVE_CARTELLE,
            &vec![
                "C:\\Musica".to_owned(),
                format!("lontano://{}/9", chiave(PC)),
            ],
        )
        .expect("cartelle");

        // Backup: i brani con statistiche sono solo quelli di qui, le cartelle solo
        // quelle vere, e la playlist porta la chiave del brano remoto.
        let stato = backup::stato_locale(&c).expect("stato locale");
        let chiavi: Vec<&str> = stato.tracks.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(chiavi, vec![riga(1).track_key.as_str()], "{chiavi:?}");
        assert_eq!(stato.roots, vec!["C:\\Musica".to_owned()]);
        assert_eq!(
            stato.playlists.first().map(|p| p.members.len()),
            Some(2),
            "le playlist che le contengono viaggiano per chiave"
        );
        let dentro = backup::snapshot(&stato, BTreeMap::new(), BTreeMap::new());
        let json = serde_json::to_string(&dentro).expect("in JSON");
        assert!(!json.contains("lontano://"), "{json}");
        assert!(!json.contains(&chiave(PC)), "{json}");

        // Profilo: né cronologia né copertine di una riga specchio.
        let biblioteca = crate::profilo::biblioteca::raccogli(&c).expect("biblioteca");
        assert!(
            biblioteca.cronologia.is_empty(),
            "{:?}",
            biblioteca.cronologia
        );
        let raccolto = crate::profilo::raccogli(
            &mut c,
            "telefono",
            BTreeMap::new(),
            BTreeMap::new(),
            1_000,
            &|| Ok("libreria-di-prova".to_owned()),
        )
        .expect("raccolta");
        let json = serde_json::to_string(&raccolto.biblioteca).expect("in JSON");
        assert!(!json.contains("lontano://"), "{json}");
        assert!(!json.contains(&chiave(PC)), "{json}");
    }

    // ── i file ──────────────────────────────────────────────────────────────────

    #[test]
    fn una_riga_specchio_non_e_un_file() {
        use crate::files::{LocalFiles, MusicFiles};
        let percorso = format!("lontano://{}/12", chiave(PC));

        // Aprire dal disco: no, e con un codice che dice perché.
        let aperto = LocalFiles.open(&percorso);
        assert_eq!(
            aperto.err().map(|e| e.code().kind()),
            Some(ErrorCodeKind::LontanoNonQui)
        );
        // Scrivere i tag: no, prima di aprire niente.
        let scritto = crate::scrittura::sola_lettura(Path::new(&percorso));
        assert_eq!(
            scritto.err().map(|e| e.code().kind()),
            Some(ErrorCodeKind::LontanoNonQui)
        );
        // Dove sta: di rete, e non si sa se c'è — non «sparito».
        assert_eq!(
            crate::volumi::dalla_forma(Path::new(&percorso)),
            Some(crate::volumi::Collocazione::DiRete)
        );
        assert_eq!(
            crate::volumi::esistenza(&percorso),
            crate::volumi::Esistenza::Ignota
        );

        // Un file di playlist è un elenco di file: fuori, come i brani del cloud.
        let c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        playlist_con(&c, "sera", &[1, id_specchio(&c, 2)]);
        let id: i64 = c
            .query_row("SELECT id FROM playlists", [], |r| r.get(0))
            .expect("playlist");
        let esportata = crate::import_playlist::esporta(
            &c,
            id,
            rymembra_domain::playlist_file::FormatoPlaylist::M3u,
        )
        .expect("esportata");
        assert_eq!((esportata.scritti, esportata.lasciati_fuori), (1, 1));
        assert!(!esportata.testo.contains("lontano://"));
    }

    #[test]
    fn le_righe_specchio_non_si_tolgono_a_mano() {
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let remoto = id_specchio(&c, 2);
        let regole = PathRules::for_current_platform();
        // Né da sole né in mezzo a righe vere: la richiesta non fa niente.
        for ids in [vec![remoto], vec![1, remoto]] {
            let errore =
                crate::library::cancella_brani(&mut c, &ids, &[], regole).expect_err("rifiutata");
            assert_eq!(errore.code().kind(), ErrorCodeKind::LontanoNonQui);
        }
        assert_eq!(n(&c, "SELECT COUNT(*) FROM tracks"), 2);
        // Le righe vere si tolgono come prima.
        let fatto =
            crate::library::cancella_brani(&mut c, &[1], &[], regole).expect("cancellazione");
        assert_eq!(fatto.tolti, 1);
    }

    #[test]
    fn le_cartelle_non_vedono_le_righe_specchio() {
        let c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2), riga(3)], ADESSO);
        let regole = PathRules::for_current_platform();
        // Il brano di qui sta nel suo albero, le righe specchio in nessuno: né sotto
        // una cartella finta «lontano:» né sotto quella del dispositivo.
        assert_eq!(
            crate::cartelle::brani_sotto_dal_database(&c, "/musica", regole).expect("albero"),
            vec![1]
        );
        for finto in [
            "lontano:".to_owned(),
            "lontano://".to_owned(),
            format!("lontano://{}", chiave(PC)),
        ] {
            assert_eq!(
                crate::cartelle::brani_sotto_dal_database(&c, &finto, regole).expect("albero"),
                Vec::<i64>::new(),
                "{finto}"
            );
            assert_eq!(
                crate::cartelle::brani_diretti_dal_database(&c, &finto, regole).expect("albero"),
                Vec::<i64>::new(),
                "{finto}"
            );
        }
    }

    // ── le passate che leggono i file, e lo scrobble ────────────────────────────

    #[test]
    fn le_passate_sui_file_non_le_prendono() {
        let c = libreria();
        brano_di_qui(&c, 1, 1);
        let conta = |c: &Connection| {
            (
                crate::enrich::quanti_mancano(c, ADESSO).expect("arricchimento"),
                crate::intensita::quanti_mancano(c, ADESSO).expect("volume"),
                crate::sonora::quanti_mancano(c, ADESSO).expect("impronta"),
                crate::intensita::avanzamento(c, ADESSO).expect("avanzamento"),
            )
        };
        let prima = conta(&c);
        assert_eq!((prima.0, prima.1, prima.2), (1, 1, 1), "il controllo");

        giro(&c, &chiave(PC), &[riga(2), riga(3), riga(4)], ADESSO);
        assert_eq!(conta(&c), prima, "tre righe specchio non cambiano niente");
    }

    /// S12-1. Non basta che una riga specchio non sia **candidata**: il disco
    /// candidato si rilegge per `album_key`, e `riallinea_le_chiavi_d_album` dà
    /// alle righe specchio proprio la chiave d'album di qui. Senza un filtro nel
    /// gruppo, titolo e artista di un altro dispositivo andavano a MusicBrainz
    /// (e a iTunes e Deezer, se accesi) e la riga veniva riscritta.
    ///
    /// Il brano di qui ha una `album_key` **esplicita e non vuota**: senza,
    /// né `candidati` né il riallineamento (che chiedono `album_key IS NOT NULL
    /// AND <> ''`) farebbero niente, e la prova passerebbe per la ragione sbagliata.
    #[test]
    fn il_gruppo_d_arricchimento_non_porta_le_righe_specchio() {
        let c = libreria();
        brano_di_qui(&c, 1, 1);
        c.execute(
            "UPDATE tracks SET album_key = 'storage/musica|album' WHERE id = 1",
            [],
        )
        .expect("chiave d'album di qui");

        giro(&c, &chiave(PC), &[riga(2), riga(3)], ADESSO);
        let remoto = id_specchio(&c, 2);
        // Il controllo: le righe specchio hanno preso la chiave d'album di qui,
        // cioè stanno nello stesso gruppo. Senza questo la prova non dice niente.
        let chiavi: Vec<String> = c
            .prepare("SELECT album_key FROM tracks ORDER BY id")
            .expect("chiavi")
            .query_map([], |r| r.get::<_, String>(0))
            .expect("chiavi")
            .map(|r| r.expect("chiave"))
            .collect();
        assert_eq!(
            chiavi,
            vec!["storage/musica|album"; 3],
            "le righe specchio prendono la chiave d'album di qui"
        );

        let gruppi = crate::enrich::candidati(&c, ADESSO, 10).expect("candidati");
        assert_eq!(gruppi.len(), 1, "il disco di qui è candidato");
        let porta: Vec<i64> = gruppi[0].brani.iter().map(|b| b.brano.id).collect();
        assert_eq!(
            porta,
            vec![1],
            "il gruppo ha solo la riga di qui, non {remoto}"
        );
        assert!(
            gruppi[0]
                .brani
                .iter()
                .all(|b| !crate::specchio::e_specchio(&b.path)),
            "nessun percorso lontano:// nel gruppo"
        );
        // E la lettura diretta del gruppo, la strada di «Dimentica l'arricchimento»
        // e di chi rilegge un disco, dice lo stesso.
        let letto = crate::enrich::leggi_gruppo(&c, "storage/musica|album")
            .expect("gruppo")
            .expect("c'è");
        assert_eq!(letto.brani.len(), 1);
    }

    /// S12-1, «Dimentica l'arricchimento»: una riga specchio che una build di
    /// prima ha annotato non finisce fra i brani d'immagine (che
    /// `registra_dimenticati` non aggiorna e conta fra i «falliti» per sempre).
    #[test]
    fn dimentica_l_arricchimento_non_conta_le_righe_specchio() {
        let mut c = libreria();
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let remoto = id_specchio(&c, 2);
        c.execute(
            "INSERT INTO track_meta_arricchita (track_id, campi, fonte, confidenza, set_at)
             VALUES (?1, '{}', 'mb-release', 0.9, 1)",
            [remoto],
        )
        .expect("annotazione di una build di prima");

        let da_fare = crate::enrich::da_dimenticare(&c).expect("da dimenticare");
        let riletti = crate::enrich::rileggi_i_file(da_fare);
        let conto = crate::enrich::registra_dimenticati(&mut c, riletti).expect("ritorno");
        assert_eq!(
            (conto.riportati, conto.falliti),
            (0, 0),
            "la riga specchio non è un brano da riportare, né uno che non si riporta"
        );
    }

    /// S12-1, il lato della scrittura: anche se una scrittura per una riga
    /// specchio arrivasse a `registra` (un gruppo letto prima del giro che ha
    /// riallineato le chiavi), la riga non si riscrive né si annota. Il
    /// riconciliatore non la rimetterebbe a posto: confronta l'impronta in arrivo
    /// con quella memorizzata, che non è cambiata.
    #[test]
    fn l_arricchimento_non_riscrive_una_riga_specchio() {
        let mut c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let remoto = id_specchio(&c, 2);
        let prima: (String, String, String, Option<String>) = c
            .query_row(
                "SELECT title, track_key, album_key, enrich_status FROM tracks WHERE id = ?1",
                [remoto],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("riga specchio");

        let scrittura = crate::enrich::Scrittura {
            track_id: remoto,
            path: crate::specchio::percorso(&chiave(PC), 2),
            campi: Fields {
                title: Some("Titolo di MusicBrainz".to_owned()),
                mb_recording_id: Some("rec-1".to_owned()),
                ..Fields::default()
            },
            copertina: None,
            fonte: "mb-release",
            confidenza: 0.95,
            originali: crate::enrich::TagOriginali::default(),
        };
        let decisione = crate::enrich::Decisione {
            album_key: prima.2.clone(),
            astensioni: vec![(remoto, Verdetto::DaRivedere)],
            ..crate::enrich::Decisione::default()
        };
        let esiti = [crate::enrich::EsitoFile {
            track_id: remoto,
            copertina: None,
            copertina_da: None,
            applicato: true,
        }];
        let tx = c.transaction().expect("transazione");
        let conto = crate::enrich::registra(&tx, &decisione, &[scrittura], &esiti, ADESSO)
            .expect("registrazione");
        tx.commit().expect("commit");

        assert_eq!(conto.applicati, 0, "nessuna riga specchio riscritta");
        assert_eq!(conto.astenuti, 0, "né segnata come «da rivedere»");
        let dopo: (String, String, String, Option<String>) = c
            .query_row(
                "SELECT title, track_key, album_key, enrich_status FROM tracks WHERE id = ?1",
                [remoto],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("riga specchio");
        assert_eq!(
            dopo, prima,
            "la riga specchio resta la copia del proprietario"
        );
        assert_eq!(
            n(&c, "SELECT COUNT(*) FROM track_meta_arricchita"),
            0,
            "e non c'è un'annotazione che «Dimentica» conterebbe fra i falliti"
        );
    }

    #[test]
    fn una_riga_specchio_non_si_scrobbla_da_qui() {
        use crate::scrobble;
        use rymembra_domain::scrobble::Servizio;
        let c = libreria();
        brano_di_qui(&c, 1, 1);
        giro(&c, &chiave(PC), &[riga(2)], ADESSO);
        let remoto = id_specchio(&c, 2);
        // Il brano vero si scrobbla; la riga specchio no, né da sola né dalla
        // cronologia («sta suonando» compreso: passa da `ascolto_di`).
        assert!(
            scrobble::ascolto_di(&c, 1, 1_700_000_000_000)
                .expect("brano")
                .is_some()
        );
        assert!(
            scrobble::ascolto_di(&c, remoto, 1_700_000_000_000)
                .expect("riga")
                .is_none()
        );
        for id in [1, remoto] {
            c.execute(
            "INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?1, 1700000000, 90000)",
            [id],
        )
        .expect("cronologia");
        }
        let accodati =
            scrobble::accoda_cronologia(&c, Servizio::ListenBrainz, None, 0).expect("cronologia");
        assert_eq!(accodati, 1, "solo l'ascolto del brano vero");
    }
}
