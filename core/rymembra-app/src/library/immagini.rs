//! Le immagini CD: i brani di un'immagine letti col suo foglio `.cue`, la cache dei fogli
//! (`fogli_cue`), le immagini rinominate e la scrittura delle due memorie della scansione.

use super::*;

/// I fotogrammi CD (1/75 s) in millisecondi, per difetto.
fn fotogrammi_in_ms(fotogrammi: u32) -> i64 {
    i64::from(fotogrammi).saturating_mul(40).div_euclid(3)
}

/// Legge i brani di un'immagine CD: l'immagine si apre **una volta**, e ne
/// escono le righe dei brani chiesti, nell'ordine chiesto.
///
/// # Da dove viene ogni campo
///
/// - **Dall'immagine**, una per tutti: frequenza, canali, bitrate, formato,
///   copertina incorporata, e la durata dell'ultimo brano, che finisce dove
///   finisce il file.
/// - **Dal foglio, con i tag dell'immagine come ripiego**: album (`TITLE` del
///   disco), artista dell'album (`PERFORMER` del disco), anno (`REM DATE`),
///   genere, numero del disco. Un foglio scritto da EAC ce li ha quasi sempre;
///   uno scritto a mano no, e allora vale quel che il rip ha messo nei tag.
/// - **Dal foglio e basta**: titolo e interprete del brano — in mancanza
///   «Traccia NN» e l'interprete del disco —, il numero (`TRACK`), inizio e
///   fine in fotogrammi, e la durata del brano: `(fine - inizio) / 75` secondi.
///
/// Non passano ai brani i campi che nell'immagine riguardano il disco intero e
/// su un brano mentirebbero: il commento, il testo, i battiti, la tonalità,
/// l'identificativo MusicBrainz della registrazione e il guadagno ReplayGain
/// «di traccia» — che su un'immagine è misurato sull'intero disco, e quindi è
/// semmai quello dell'album.
///
/// Il testo in particolare (`tracks.lyrics`) resta **sempre vuoto** su una
/// riga d'immagine, all'inserimento come a ogni riscrittura: è la stessa regola
/// di un file intero — la colonna dice quel che il file dice, e
/// [`update_track`] la riscrive dai tag ogni volta — applicata a un file che di
/// un brano solo non dice niente. Un testo incorporato nell'immagine
/// comparirebbe identico su tutti i quattordici brani.
///
/// # Dimensione e data
///
/// Quelle della voce virtuale, **esattamente**: la dimensione dell'immagine e
/// la data più recente fra il `.cue` e l'immagine (vedi
/// [`rymembra_domain::cue::espandi_immagini`]). Sono i due numeri che il piano
/// confronta alla passata dopo, e uno solo diverso farebbe rileggere ogni
/// brano d'immagine a ogni scansione.
///
/// # La chiave di contenuto
///
/// Si calcola con gli stessi ingressi di un file, presi dal foglio: interprete
/// (del brano, del disco, dei tag dell'immagine), titolo del brano **com'è
/// scritto nel foglio** — senza il ripiego «Traccia NN» —, album, durata **del
/// brano** e dimensione dell'immagine. Con interprete e titolo è
/// `interprete|titolo|album`, la stessa che avrebbe quel brano rippato a file
/// separati. Senza uno dei due la chiave ripiegherebbe su
/// `durata|dimensione`, e la dimensione dell'immagine è la stessa per tutti i
/// brani del disco: le si aggiunge allora il numero del brano
/// (`durata|dimensione|NN`). Nessun ingresso dipende dal percorso, ed è quel
/// che permette di riconoscere un'immagine spostata con il suo `.cue`: i brani
/// tengono identificativo, ascolti e playlist.
pub(super) fn read_image_tracks(
    files: &dyn MusicFiles,
    covers: &CoverStore,
    abbinamento: &Abbinamento,
    voci: &[DiscoveredFile],
) -> Result<Vec<TrackRow>, AppError> {
    use rymembra_domain::album::{UNKNOWN_ALBUM, UNKNOWN_ARTIST};
    use rymembra_domain::keys::normalize_key;

    let tags = read_tags(files, &abbinamento.immagine.path)?;
    let foglio = &abbinamento.foglio;
    let pulito = |valore: Option<&String>| crate::metadata::clean(valore.map(String::as_str));
    let durata_immagine = i64::try_from(tags.duration_ms).unwrap_or(0);

    let album_grezzo = pulito(foglio.titolo.as_ref()).or_else(|| tags.album.clone());
    let album = album_grezzo
        .clone()
        .unwrap_or_else(|| UNKNOWN_ALBUM.to_owned());
    let interprete_del_disco = pulito(foglio.interprete.as_ref());
    let album_artist = interprete_del_disco
        .clone()
        .or_else(|| tags.album_artist.clone());
    // `REM DATE` è testo: `1999`, `1999/05/12`, `1999-05`. Le prime quattro
    // cifre, quando lo sono.
    let year = foglio
        .data
        .as_deref()
        .and_then(|data| data.get(..4))
        .filter(|anno| anno.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|anno| anno.parse::<i64>().ok())
        .or_else(|| tags.year.map(i64::from));
    let genre = pulito(foglio.genere.as_ref()).or_else(|| tags.genre.clone());
    let disc_number = foglio.disco.or(tags.disc_number).map(i64::from);

    // La copertina una volta per l'immagine: la stessa per tutti i brani, e
    // salvarla quattordici volte vorrebbe dire contarla quattordici volte fra
    // quelle ricodificate.
    let (cover, mut copertina_persa) = match tags.cover.as_ref() {
        None => (None, None),
        Some(incorporata) => match covers.store(&incorporata.data, CoverSource::Tag) {
            Ok(salvata) => (Some(salvata), None),
            Err(err) => (None, Some(err)),
        },
    };

    let mut righe = Vec::with_capacity(voci.len());
    for (posto, voce) in voci.iter().enumerate() {
        let fuori = || {
            AppError::new(ErrorCode::MetadataTagReadFailed {
                path: Some(voce.path.clone()),
            })
            .with_cause("il foglio non ha questo brano".to_owned())
        };
        let (_, numero) = brano_d_immagine(&voce.path).ok_or_else(fuori)?;
        let indice = foglio
            .tracce
            .iter()
            .position(|traccia| traccia.numero == numero)
            .ok_or_else(fuori)?;
        let traccia = foglio.tracce.get(indice).ok_or_else(fuori)?;
        let fine = foglio.fine_cd(indice);
        let duration_ms = match fine {
            Some(fine) => fotogrammi_in_ms(fine.saturating_sub(traccia.inizio_cd)),
            None => durata_immagine.saturating_sub(fotogrammi_in_ms(traccia.inizio_cd)),
        }
        .max(0);

        let titolo_grezzo = pulito(traccia.titolo.as_ref());
        let artista_grezzo = pulito(traccia.interprete.as_ref())
            .or_else(|| interprete_del_disco.clone())
            .or_else(|| tags.artist.clone());
        let file_size = i64::try_from(voce.size_bytes).unwrap_or(i64::MAX);

        let chiave = ContentKey::compute(ContentKeyInput {
            artist: artista_grezzo.as_deref(),
            title: titolo_grezzo.as_deref(),
            album: album_grezzo.as_deref(),
            duration_ms: Some(duration_ms),
            file_size: Some(file_size),
        })
        .into_string();
        let content_key = if normalize_key(artista_grezzo.as_deref()).is_empty()
            || normalize_key(titolo_grezzo.as_deref()).is_empty()
        {
            format!("{chiave}|{numero:02}")
        } else {
            chiave
        };

        let title = titolo_grezzo.unwrap_or_else(|| format!("Traccia {numero:02}"));
        let artist = artista_grezzo.unwrap_or_else(|| UNKNOWN_ARTIST.to_owned());
        let track_key = TrackKey::compute(TrackKeyInput {
            artist: Some(&artist),
            title: Some(&title),
            album: Some(&album),
        })
        .into_string();
        // La cartella del percorso virtuale è quella del `.cue`: il nome dopo
        // l'ultimo separatore è `Album.cue|03`, e la chiave d'album lo toglie
        // come toglie il nome di un file.
        let album_key = album_group_key(&album, &voce.path);

        // Dal secondo brano in poi la copertina è «già presente»: l'ha
        // salvata il primo, e la stessa immagine non si conta due volte.
        let copertina = cover.clone().map(|mut salvata| {
            if posto > 0 {
                salvata.already_present = true;
            }
            salvata
        });

        righe.push(TrackRow {
            path: voce.path.clone(),
            track_key,
            content_key: Some(content_key),
            title,
            artist,
            album: album.clone(),
            album_artist: album_artist.clone(),
            album_key,
            year,
            track_number: Some(i64::from(numero)),
            disc_number,
            genre: genre.clone(),
            duration_ms,
            bpm: None,
            musical_key: None,
            comment: None,
            lyrics: None,
            cover: copertina,
            copertina_persa: copertina_persa.take(),
            bitrate: tags.bitrate.map(i64::from),
            sample_rate: tags.sample_rate.map(i64::from),
            channels: tags.channels.map(i64::from),
            codec: tags.codec.clone(),
            file_size,
            date_modified: voce.modified_ms,
            replaygain_track_db: None,
            replaygain_album_db: tags
                .replaygain_album_db
                .or(tags.replaygain_track_db)
                .map(f64::from),
            mb_recording_id: None,
            mb_release_group_id: tags.mb_release_group_id.clone(),
            mb_release_id: tags.mb_release_id.clone(),
            immagine: Some(abbinamento.immagine.path.clone()),
            inizio_cd: Some(i64::from(traccia.inizio_cd)),
            fine_cd: fine.map(i64::from),
        });
    }
    Ok(righe)
}

/// Un foglio come lo ricorda la cache `fogli_cue`.
pub(super) struct FoglioRicordato {
    /// La dimensione che il `.cue` aveva quando lo si è letto.
    pub(super) dimensione: u64,
    /// La data che aveva.
    pub(super) modificato_ms: i64,
    /// Quel che diceva; `None` se non descriveva un'immagine.
    pub(super) foglio: Option<Foglio>,
}

/// Un foglio letto in questa passata, da mettere nella cache.
pub(super) struct FoglioLetto {
    /// La chiave di percorso del `.cue`.
    pub(super) chiave: String,
    /// La sua dimensione.
    pub(super) dimensione: u64,
    /// La sua data.
    pub(super) modificato_ms: i64,
    /// Quel che dice.
    pub(super) foglio: Option<Foglio>,
}

/// Il foglio nella forma in cui sta nella colonna `fogli_cue.foglio`: JSON.
///
/// Una copia dei campi del dominio con i suoi nomi, e non un `derive` sul
/// dominio: il dominio non serializza niente da sé (vedi il `Cargo.toml` di
/// questa cassa). I nomi sono quelli del dominio perché sono il formato: se il
/// dominio aggiunge un campo, un foglio in cache senza quel campo si legge lo
/// stesso (`serde(default)`) e il campo resta vuoto finché il `.cue` non si
/// rilegge — al più tardi con «Rileggi tutti i tag», che svuota la cache.
#[derive(serde::Serialize, serde::Deserialize)]
struct FoglioSalvato {
    file: String,
    #[serde(default)]
    titolo: Option<String>,
    #[serde(default)]
    interprete: Option<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    genere: Option<String>,
    #[serde(default)]
    disco: Option<u32>,
    tracce: Vec<TracciaSalvata>,
}

/// Un brano del foglio salvato. Vedi [`FoglioSalvato`].
#[derive(serde::Serialize, serde::Deserialize)]
struct TracciaSalvata {
    numero: u8,
    #[serde(default)]
    titolo: Option<String>,
    #[serde(default)]
    interprete: Option<String>,
    inizio_cd: u32,
}

impl From<&Foglio> for FoglioSalvato {
    fn from(foglio: &Foglio) -> Self {
        Self {
            file: foglio.file.clone(),
            titolo: foglio.titolo.clone(),
            interprete: foglio.interprete.clone(),
            data: foglio.data.clone(),
            genere: foglio.genere.clone(),
            disco: foglio.disco,
            tracce: foglio
                .tracce
                .iter()
                .map(|traccia| TracciaSalvata {
                    numero: traccia.numero,
                    titolo: traccia.titolo.clone(),
                    interprete: traccia.interprete.clone(),
                    inizio_cd: traccia.inizio_cd,
                })
                .collect(),
        }
    }
}

impl From<FoglioSalvato> for Foglio {
    fn from(salvato: FoglioSalvato) -> Self {
        Self {
            file: salvato.file,
            titolo: salvato.titolo,
            interprete: salvato.interprete,
            data: salvato.data,
            genere: salvato.genere,
            disco: salvato.disco,
            tracce: salvato
                .tracce
                .into_iter()
                .map(|traccia| rymembra_domain::cue::TracciaCue {
                    numero: traccia.numero,
                    titolo: traccia.titolo,
                    interprete: traccia.interprete,
                    inizio_cd: traccia.inizio_cd,
                })
                .collect(),
        }
    }
}

/// La cache dei fogli, per chiave di percorso del `.cue`.
///
/// Una riga che non si rilegge — un JSON rotto, scritto da chissà chi — si
/// salta: il suo `.cue` si rilegge, e la riga si riscrive.
pub(super) fn fogli_ricordati(
    connection: &Connection,
) -> Result<std::collections::HashMap<String, FoglioRicordato>, AppError> {
    let mut statement = connection
        .prepare("SELECT chiave, dimensione, modificato_ms, foglio FROM fogli_cue")
        .map_err(|err| db_error("lettura dei fogli ricordati", &err))?;
    let righe = statement
        .query_map([], |riga| {
            Ok((
                riga.get::<_, String>(0)?,
                riga.get::<_, i64>(1)?,
                riga.get::<_, i64>(2)?,
                riga.get::<_, String>(3)?,
            ))
        })
        .map_err(|err| db_error("lettura dei fogli ricordati", &err))?;
    let mut fogli = std::collections::HashMap::new();
    for riga in righe {
        let (chiave, dimensione, modificato_ms, testo) =
            riga.map_err(|err| db_error("lettura dei fogli ricordati", &err))?;
        let (Ok(dimensione), Ok(foglio)) = (
            u64::try_from(dimensione),
            serde_json::from_str::<Option<FoglioSalvato>>(&testo),
        ) else {
            continue;
        };
        fogli.insert(
            chiave,
            FoglioRicordato {
                dimensione,
                modificato_ms,
                foglio: foglio.map(Foglio::from),
            },
        );
    }
    Ok(fogli)
}

/// Le chiavi dei fogli che la cache ricorda, senza i fogli: per potarla.
pub(super) fn chiavi_dei_fogli(connection: &Connection) -> Result<Vec<String>, AppError> {
    let mut statement = connection
        .prepare("SELECT chiave FROM fogli_cue")
        .map_err(|err| db_error("lettura dei fogli ricordati", &err))?;
    let righe = statement
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error("lettura dei fogli ricordati", &err))?;
    righe
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| db_error("lettura dei fogli ricordati", &err))
}

/// Le chiavi di percorso dei `.cue` che hanno già dei brani in libreria.
///
/// Per [`rymembra_domain::cue::espandi_immagini`]: fra due fogli per la stessa
/// immagine vince quello che i brani li ha già. Dalle righe e non dalla cache
/// dei fogli, perché la cache ricorda anche i fogli che hanno perso.
pub(super) fn fogli_con_brani(
    connection: &Connection,
    rules: PathRules,
) -> Result<std::collections::HashSet<String>, AppError> {
    let mut statement = connection
        .prepare("SELECT path FROM tracks WHERE immagine IS NOT NULL AND path IS NOT NULL")
        .map_err(|err| db_error("lettura dei fogli con brani", &err))?;
    let righe = statement
        .query_map([], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error("lettura dei fogli con brani", &err))?;
    let mut chiavi = std::collections::HashSet::new();
    for riga in righe {
        let percorso = riga.map_err(|err| db_error("lettura dei fogli con brani", &err))?;
        if let Some((cue, _)) = brano_d_immagine(&percorso) {
            chiavi.insert(path_key(cue, rules));
        }
    }
    Ok(chiavi)
}

/// L'immagine di ogni brano d'immagine in libreria, per identificativo.
///
/// Si legge con le righe note, nella stessa presa: serve al piano per
/// accorgersi di un'immagine rinominata. Vedi [`immagini_rinominate`].
pub(super) fn immagini_dei_brani(
    connection: &Connection,
) -> Result<std::collections::HashMap<i64, String>, AppError> {
    let mut statement = connection
        .prepare("SELECT id, immagine FROM tracks WHERE immagine IS NOT NULL")
        .map_err(|err| db_error("lettura delle immagini dei brani", &err))?;
    let righe = statement
        .query_map([], |riga| {
            Ok((riga.get::<_, i64>(0)?, riga.get::<_, String>(1)?))
        })
        .map_err(|err| db_error("lettura delle immagini dei brani", &err))?;
    righe
        .collect::<Result<_, _>>()
        .map_err(|err| db_error("lettura delle immagini dei brani", &err))
}

/// Rimette fra gli aggiornamenti i brani d'immagine che il piano dà per
/// invariati e la cui immagine, sul disco, non è più quella della riga.
///
/// # Perché il piano non li vede
///
/// Il piano confronta la voce virtuale con la riga: il percorso
/// `…\Album.cue|03`, la dimensione dell'immagine, la data più recente fra
/// foglio e immagine. Un'immagine rinominata senza toccare il foglio non cambia
/// nessuno dei tre — il foglio la ritrova lo stesso, col nome del `.cue` o con
/// le sole maiuscole diverse, e un rinomina non cambia la data — e la colonna
/// `immagine` resterebbe col nome vecchio per sempre: i brani non suonano più,
/// e «elimina dal disco» cestinerebbe il `.cue` lasciando l'immagine.
///
/// Si confronta come il piano confronta i percorsi, maiuscole comprese sotto
/// la radice: vedi [`stessa_grafia`]. Su una share sensibile alle maiuscole
/// `Disco.flac` diventato `disco.flac` è un altro nome, e il vecchio non si
/// apre.
///
/// Un brano rimesso qui si rilegge come uno cambiato, e la riga si riscrive con
/// l'immagine nuova: stesso identificativo, quindi ascolti e playlist restano.
/// Esce dalle invariate, che la guardia contro le stragi somma.
pub(super) fn immagini_rinominate(
    piano: &mut ScanPlan,
    ricognizione: &Ricognizione,
    known: &[KnownTrack],
    immagini: &std::collections::HashMap<i64, String>,
    abbinamenti: &[Abbinamento],
    rules: PathRules,
) {
    if immagini.is_empty() || abbinamenti.is_empty() {
        return;
    }
    let per_foglio: std::collections::HashMap<String, &Abbinamento> = abbinamenti
        .iter()
        .map(|abbinamento| (path_key(&abbinamento.cue.path, rules), abbinamento))
        .collect();
    // La riga di ogni percorso virtuale: fra due con la stessa chiave quella
    // che il piano tiene, la più vecchia.
    let mut righe: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for riga in known
        .iter()
        .filter(|riga| brano_d_immagine(&riga.path).is_some())
    {
        righe
            .entry(path_key(&riga.path, rules))
            .and_modify(|id| *id = (*id).min(riga.id))
            .or_insert(riga.id);
    }
    let gia: std::collections::HashSet<i64> = piano
        .to_update
        .iter()
        .map(|pendente| pendente.track_id)
        .collect();
    // Le voci scartate il piano non le ha giudicate invariate; le doppie sì,
    // la prima volta.
    let scartate: std::collections::HashSet<String> = piano
        .skipped
        .iter()
        .filter(|scarto| scarto.reason != SkipReason::Duplicate)
        .map(|scarto| path_key(&scarto.file.path, rules))
        .collect();
    let mut viste: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut in_piu: Vec<PendingUpdate> = Vec::new();
    for voce in &ricognizione.trovati {
        let Some((cue, _)) = brano_d_immagine(&voce.path) else {
            continue;
        };
        let chiave = path_key(&voce.path, rules);
        if scartate.contains(&chiave) || !viste.insert(chiave.clone()) {
            continue;
        }
        let Some(&id) = righe.get(&chiave) else {
            continue;
        };
        if gia.contains(&id) {
            continue;
        }
        let (Some(colonna), Some(abbinamento)) =
            (immagini.get(&id), per_foglio.get(&path_key(cue, rules)))
        else {
            continue;
        };
        if stessa_grafia(
            colonna,
            &abbinamento.immagine.path,
            &ricognizione.viste,
            rules,
        ) {
            continue;
        }
        in_piu.push(PendingUpdate {
            file: voce.clone(),
            track_id: id,
        });
    }
    piano.unchanged = piano.unchanged.saturating_sub(in_piu.len());
    piano.to_update.extend(in_piu);
}

/// Scrive nelle due memorie della scansione quel che la fase dei fogli e il
/// piano hanno deciso: i fogli appena letti nella cache, e via le righe di file
/// che sotto una radice vista non ci sono più.
pub(super) fn aggiorna_le_memorie(
    tx: &Transaction<'_>,
    letti: &[FoglioLetto],
    rifiuti_da_potare: &[String],
    fogli_da_potare: &[String],
) -> Result<(), AppError> {
    let mut ricorda = tx
        .prepare_cached(
            "INSERT OR REPLACE INTO fogli_cue (chiave, dimensione, modificato_ms, foglio)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(|err| db_error("cache dei fogli", &err))?;
    for letto in letti {
        let foglio = serde_json::to_string(&letto.foglio.as_ref().map(FoglioSalvato::from))
            .map_err(|err| {
                AppError::new(ErrorCode::DbQueryFailed {
                    detail: Some("cache dei fogli".to_owned()),
                })
                .with_cause(err.to_string())
            })?;
        ricorda
            .execute(rusqlite::params![
                letto.chiave,
                i64::try_from(letto.dimensione).unwrap_or(i64::MAX),
                letto.modificato_ms,
                foglio,
            ])
            .map_err(|err| db_error("cache dei fogli", &err))?;
    }
    let mut via_il_foglio = tx
        .prepare_cached("DELETE FROM fogli_cue WHERE chiave = ?1")
        .map_err(|err| db_error("cache dei fogli", &err))?;
    for chiave in fogli_da_potare {
        via_il_foglio
            .execute([chiave])
            .map_err(|err| db_error("cache dei fogli", &err))?;
    }
    let mut via_il_rifiuto = tx
        .prepare_cached("DELETE FROM file_illeggibili WHERE chiave = ?1")
        .map_err(|err| db_error("memoria dei file illeggibili", &err))?;
    for chiave in rifiuti_da_potare {
        via_il_rifiuto
            .execute([chiave])
            .map_err(|err| db_error("memoria dei file illeggibili", &err))?;
    }
    Ok(())
}
