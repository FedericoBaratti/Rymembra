//! Le prove di [`crate::sorveglia`]: l'accorpamento, il pianificatore, il motore
//! dopo le passate, e un osservatore vero su una cartella temporanea.

use super::*;

use std::path::Path;
use std::time::{Instant, SystemTime};

use rymembra_app::library::Natura;
use rymembra_domain::paths::PathRules;

use super::accorpamento::{cartelle_da_guardare, conta, natura_di};
use super::filo::{Motore, Passata, nel_diario, osserva, prossima_attesa};

#[test]
fn la_riga_del_ramo_perso_non_porta_il_percorso() {
    // `PRIVACY.md` promette che nel diario non finiscono i percorsi della
    // musica: un ramo che non si legge sul NAS è il nome di un artista e
    // di un album.
    let elenco = vec![r"\\nas\m".to_owned(), r"D:\Musica di Mario".to_owned()];
    let ramo = nel_diario(r"\\nas\m\Guccini\Radici", &elenco);
    for nome in ["nas", "Guccini", "Radici", "Mario"] {
        assert!(!ramo.contains(nome), "«{nome}» nel diario: {ramo}");
    }
    assert!(ramo.contains("cartella 1"), "{ramo}");
    assert!(ramo.contains("2 livelli"), "{ramo}");
    // La stessa cartella si riconosce fra due righe, anche scritta in
    // un'altra grafia; un'altra no.
    assert_eq!(ramo, nel_diario(r"\\NAS\m\guccini\radici\", &elenco));
    assert_ne!(ramo, nel_diario(r"\\nas\m\Guccini\Amerigo", &elenco));
    let radice = nel_diario(r"D:\Musica di Mario", &elenco);
    assert!(radice.starts_with("cartella 2 ["), "{radice}");
    assert!(!radice.contains("Mario"), "{radice}");
    let fuori = nel_diario(r"E:\Altro\Album", &elenco);
    assert!(
        !fuori.contains("Altro") && !fuori.contains("Album"),
        "{fuori}"
    );
}
use std::path::PathBuf;

/// Un cambiamento nuovo: creato, o una cartella da riguardare.
fn nuovo(percorso: &str) -> Cambiamento {
    Cambiamento {
        percorso: percorso.to_owned(),
        nuovo: true,
    }
}

/// Un cambiamento che si guarda dalla cartella che lo contiene.
fn vecchio(percorso: &str) -> Cambiamento {
    Cambiamento {
        percorso: percorso.to_owned(),
        nuovo: false,
    }
}

#[test]
fn molti_avvisi_fanno_una_scansione_sola() {
    let zero = Instant::now();
    let mut attesa = Accorpatore::default();
    assert_eq!(attesa.manca(zero), None);
    // Una copia: venti file, uno ogni cento millisecondi.
    for n in 0..20_u64 {
        let quando = zero + Duration::from_millis(100 * n);
        attesa.avvisati([nuovo(&format!(r"C:\M\A\{n:02}.flac"))], quando);
        assert!(
            !attesa.pronto(quando),
            "durante la raffica non si scansiona"
        );
    }
    let ultimo = zero + Duration::from_millis(1_900);
    assert!(!attesa.pronto(ultimo + Duration::from_millis(1_999)));
    assert!(
        attesa.pronto(ultimo + ACCORPAMENTO),
        "il silenzio è arrivato"
    );
    let cambiati = attesa.prendi(ultimo + ACCORPAMENTO);
    assert_eq!(cambiati.len(), 20, "tutti in una volta sola");
    assert_eq!(cambiati.first(), Some(&nuovo(r"C:\M\A\00.flac")));
    assert_eq!(attesa.manca(ultimo), None, "e l'attesa ricomincia da vuota");
    assert!(!attesa.pronto(ultimo + ACCORPAMENTO * 10));
    assert!(attesa.prendi(ultimo + ACCORPAMENTO * 10).is_empty());
}

#[test]
fn una_raffica_senza_fine_non_rimanda_per_sempre() {
    let zero = Instant::now();
    let mut attesa = Accorpatore::default();
    let mut quando = zero;
    while quando < zero + ATTESA_MASSIMA {
        attesa.avvisati([vecchio("x")], quando);
        assert!(!attesa.pronto(quando));
        quando += Duration::from_millis(500);
    }
    assert!(
        attesa.pronto(zero + ATTESA_MASSIMA),
        "al tetto si scansiona"
    );
}

#[test]
fn con_il_turno_occupato_si_riprova_fra_poco() {
    let zero = Instant::now();
    let mut attesa = Accorpatore::default();
    attesa.avvisati([vecchio("a")], zero);
    let dopo = zero + ATTESA_MASSIMA + ACCORPAMENTO;
    assert!(attesa.pronto(dopo));
    let cambiati = attesa.prendi(dopo);
    attesa.rimetti(cambiati, dopo);
    assert!(
        !attesa.pronto(dopo),
        "non si gira a vuoto sul turno occupato"
    );
    assert_eq!(attesa.manca(dopo), Some(ACCORPAMENTO));
    assert!(attesa.pronto(dopo + ACCORPAMENTO));
    assert_eq!(attesa.prendi(dopo + ACCORPAMENTO), vec![vecchio("a")]);
    // Rimettere niente non accende nessuna attesa.
    let mut vuota = Accorpatore::default();
    vuota.rimetti(Vec::new(), dopo);
    vuota.rinvia(
        Vec::new(),
        dopo,
        SONDAGGIO,
        PathRules::for_current_platform(),
    );
    assert_eq!(vuota.manca(dopo), None);
}

#[test]
fn quel_che_non_ha_risposto_torna_col_suo_rinvio_senza_fermare_il_resto() {
    let regole = PathRules {
        case_insensitive: true,
    };
    let zero = Instant::now();
    let mut attesa = Accorpatore::default();
    attesa.rinvia(vec![nuovo(r"\\nas\m")], zero, SONDAGGIO, regole);
    assert!(
        !attesa.pronto(zero + ACCORPAMENTO * 10),
        "un NAS spento non si riprova a ogni raffica"
    );
    assert_eq!(attesa.manca(zero), Some(SONDAGGIO));
    // Un album copiato sul disco di casa intanto non aspetta il rinvio, e
    // il rinvio non parte con lui.
    attesa.avvisati([vecchio(r"C:\M\A\01.flac")], zero);
    let silenzio = zero + ACCORPAMENTO;
    assert!(attesa.pronto(silenzio));
    assert_eq!(attesa.prendi(silenzio), vec![vecchio(r"C:\M\A\01.flac")]);
    assert_eq!(attesa.manca(silenzio), Some(SONDAGGIO - ACCORPAMENTO));
    // Un minuto dopo anche il disco di casa non risponde: torna indietro.
    let minuto = zero + Duration::from_secs(60);
    attesa.rinvia(vec![nuovo(r"C:\M")], minuto, SONDAGGIO, regole);
    // Il NAS scade per primo, da solo, e torna indietro ancora spento.
    assert_eq!(attesa.prendi(zero + SONDAGGIO), vec![nuovo(r"\\nas\m")]);
    let di_nuovo = zero + SONDAGGIO + Duration::from_secs(3);
    attesa.rinvia(vec![nuovo(r"\\nas\m")], di_nuovo, SONDAGGIO, regole);
    // Il disco di casa non si sposta: ognuno il suo rinvio. Con un rinvio
    // per tutti, qui avrebbe aspettato il NAS — e ogni cinque minuti di
    // nuovo, per sempre.
    assert_eq!(attesa.manca(di_nuovo), Some(minuto + SONDAGGIO - di_nuovo));
    assert_eq!(attesa.prendi(minuto + SONDAGGIO), vec![nuovo(r"C:\M")]);
    // Lo stesso percorso due volte è una voce sola, col rinvio più lontano.
    attesa.rinvia(vec![nuovo(r"\\NAS\m")], di_nuovo, CONTROLLO, regole);
    assert!(attesa.prendi(di_nuovo + CONTROLLO).is_empty());
    assert_eq!(attesa.prendi(di_nuovo + SONDAGGIO), vec![nuovo(r"\\nas\m")]);
    assert_eq!(attesa.manca(di_nuovo + SONDAGGIO), None);
}

#[test]
fn un_riallineamento_si_porta_via_quel_che_aspetta_sotto_la_sua_radice() {
    let zero = Instant::now();
    let regole = PathRules {
        case_insensitive: true,
    };
    let mut attesa = Accorpatore::default();
    attesa.avvisati(
        [vecchio(r"\\nas\m\A\01.flac"), vecchio(r"C:\M\B\02.flac")],
        zero,
    );
    attesa.rinvia(vec![nuovo(r"\\NAS\m\C")], zero, SONDAGGIO, regole);
    assert_eq!(
        attesa.togli_sotto(r"\\nas\m", regole),
        vec![vecchio(r"\\nas\m\A\01.flac"), nuovo(r"\\NAS\m\C")]
    );
    assert_eq!(
        attesa.manca(zero),
        Some(ACCORPAMENTO),
        "il resto aspetta come prima"
    );
    assert_eq!(
        attesa.prendi(zero + ACCORPAMENTO),
        vec![vecchio(r"C:\M\B\02.flac")]
    );
    assert_eq!(
        attesa.manca(zero + ACCORPAMENTO),
        None,
        "e un rinvio vuoto non resta acceso"
    );
    // Una radice sotto cui non aspetta niente non porta via niente.
    attesa.avvisati([vecchio(r"C:\M\B\03.flac")], zero);
    assert!(attesa.togli_sotto(r"\\nas\m", regole).is_empty());
    assert_eq!(attesa.manca(zero), Some(ACCORPAMENTO));
}

#[test]
fn le_copertine_e_i_testi_non_valgono_una_scansione() {
    assert!(conta(Path::new(r"C:\M\A\01.flac")));
    assert!(conta(Path::new(r"C:\M\A")), "una cartella sì");
    assert!(
        conta(Path::new(r"C:\M\Vol. 2")),
        "anche con un punto nel nome"
    );
    assert!(
        conta(Path::new(r"C:\M\A\Album.CUE")),
        "il foglio di un'immagine CD cambia i suoi brani"
    );
    assert!(conta(Path::new(r"C:\M\A\CDImage.ape")));
    assert!(!conta(Path::new(r"C:\M\A\cover.JPG")));
    assert!(!conta(Path::new(r"C:\M\A\01.lrc")));
    assert!(!conta(Path::new(r"C:\M\A\01.flac.part")));
}

#[test]
fn gli_avvisi_diventano_cambiamenti() {
    use notify::event::{AccessKind, RenameMode};
    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind};
    use notify::{Event, EventKind};
    // Un disco finto: «Album» è una cartella, tutto il resto un file.
    let natura = |percorso: &Path| {
        if percorso.to_string_lossy().ends_with("Album") {
            Natura::Cartella
        } else {
            Natura::File
        }
    };
    let avviso = |genere: EventKind, percorsi: &[&str]| {
        let evento = percorsi
            .iter()
            .fold(Event::new(genere), |evento, percorso| {
                evento.add_path(PathBuf::from(percorso))
            });
        cambiamenti_dell_avviso(&evento, &natura)
    };
    let album = r"\\nas\m\Guccini\Album";
    let brano = r"\\nas\m\Guccini\Album\01.flac";

    // Una cartella creata è nuova: si guarderà da sé, non dall'artista.
    assert_eq!(
        avviso(EventKind::Create(CreateKind::Any), &[album]),
        vec![nuovo(album)]
    );
    // Anche rinominata verso; il nome di prima no.
    assert_eq!(
        avviso(
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
            &[album]
        ),
        vec![nuovo(album)]
    );
    assert_eq!(
        avviso(
            EventKind::Modify(ModifyKind::Name(RenameMode::From)),
            &[r"\\nas\m\Guccini\Vecchio"]
        ),
        vec![vecchio(r"\\nas\m\Guccini\Vecchio")]
    );
    assert_eq!(
        avviso(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &[r"\\nas\m\Guccini\Vecchio", album]
        ),
        vec![vecchio(r"\\nas\m\Guccini\Vecchio"), nuovo(album)]
    );
    assert_eq!(
        avviso(EventKind::Remove(RemoveKind::Any), &[album]),
        vec![vecchio(album)]
    );
    // La modifica di una cartella si salta: l'ha cambiata un figlio, che
    // ha il suo avviso. È quel che il NAS manda a ogni file copiato.
    for modifica in [
        ModifyKind::Any,
        ModifyKind::Metadata(MetadataKind::WriteTime),
        ModifyKind::Data(DataChange::Any),
    ] {
        assert!(
            avviso(EventKind::Modify(modifica), &[album]).is_empty(),
            "{modifica:?}"
        );
    }
    // La modifica di un file no.
    assert_eq!(
        avviso(EventKind::Modify(ModifyKind::Any), &[brano]),
        vec![vecchio(brano)]
    );
    // Il foglio di un'immagine conta; la copertina e gli accessi no.
    let foglio = r"\\nas\m\Guccini\Album\Album.cue";
    assert_eq!(
        avviso(EventKind::Modify(ModifyKind::Any), &[foglio]),
        vec![vecchio(foglio)]
    );
    assert!(
        avviso(
            EventKind::Create(CreateKind::File),
            &[r"\\nas\m\Guccini\Album\cover.jpg"]
        )
        .is_empty()
    );
    assert!(avviso(EventKind::Access(AccessKind::Any), &[brano]).is_empty());
}

#[test]
fn la_natura_di_un_percorso() {
    let cartella = tempfile::tempdir().expect("cartella");
    let dentro = |nome: &str| cartella.path().join(nome);
    std::fs::create_dir(dentro("Album")).expect("cartella");
    std::fs::write(dentro("note.txt"), b"x").expect("file");
    assert_eq!(natura_di(&dentro("Album")), Natura::Cartella);
    assert_eq!(natura_di(&dentro("note.txt")), Natura::File);
    assert_eq!(natura_di(&dentro("manca")), Natura::Assente);
    // Un brano o un foglio si riconoscono dal nome, senza chiedere: questo
    // non esiste, e il disco avrebbe risposto «assente».
    assert_eq!(natura_di(&dentro("manca.flac")), Natura::File);
    assert_eq!(natura_di(&dentro("manca.cue")), Natura::File);
}

#[test]
fn l_interruttore_decide_chi_si_guarda() {
    use Collocazione::{DiRete, Ignota, Locale};
    for dove in [Locale, DiRete, Ignota] {
        assert_eq!(come_sorvegliare(Modo::Spenta, dove), Come::Ferma);
    }
    assert_eq!(come_sorvegliare(Modo::Locali, Locale), Come::Avvisi);
    assert_eq!(come_sorvegliare(Modo::Locali, DiRete), Come::Ferma);
    assert_eq!(
        come_sorvegliare(Modo::Locali, Ignota),
        Come::NonRaggiungibile,
        "un disco che non risponde si riprova"
    );
    // Con «tutte» gli avvisi si vogliono ovunque, rete compresa: è il
    // filo a ripiegare sul solo riallineamento se non si armano.
    for dove in [Locale, DiRete, Ignota] {
        assert_eq!(come_sorvegliare(Modo::Tutte, dove), Come::Avvisi);
    }
    // Di serie, tutte; un nome sconosciuto non spegne niente; chi aveva
    // scelto resta com'era.
    assert_eq!(Modo::dal_salvato(None), Modo::Tutte);
    assert_eq!(Modo::dal_salvato(Some("domani")), Modo::Tutte);
    assert_eq!(Modo::dal_salvato(Some("locali")), Modo::Locali);
    assert_eq!(Modo::dal_salvato(Some("spenta")), Modo::Spenta);
    for modo in [Modo::Spenta, Modo::Locali, Modo::Tutte] {
        assert_eq!(Modo::da_testo(modo.come_testo()), Some(modo));
    }
    // E il passo che ne viene.
    assert_eq!(Ritmo::di(Come::Avvisi, Locale), Ritmo::Locale);
    assert_eq!(Ritmo::di(Come::Avvisi, DiRete), Ritmo::Rete);
    assert_eq!(
        Ritmo::di(Come::Avvisi, Ignota),
        Ritmo::Rete,
        "una lettera che non si sa dove porti si tratta da rete"
    );
    assert_eq!(
        Ritmo::di(Come::Sondaggio, DiRete),
        Ritmo::SoloRiallineamento
    );
    assert_eq!(Ritmo::di(Come::Ferma, DiRete), Ritmo::Nessuno);
    assert_eq!(Ritmo::di(Come::NonRaggiungibile, Locale), Ritmo::Nessuno);
}

// ── il pianificatore ──

const NAS: &str = r"\\nas\musica";
const CASA: &str = r"C:\Musica";
const USB: &str = r"E:\Musica";

/// Un pianificatore con l'elenco dell'avvio letto e il risveglio finito.
fn piano_avviato(radici: &[(&str, Ritmo)], adesso: Instant) -> Pianificatore {
    let mut piano = Pianificatore::default();
    for (radice, ritmo) in radici {
        piano.imposta(radice, *ritmo, adesso);
    }
    piano.elenco_letto();
    piano.avvio();
    piano
}

/// Fa tutti i riallineamenti dovuti da `adesso`, uno per respiro, come il
/// filo, e dice quali.
fn fai_tutti(piano: &mut Pianificatore, adesso: Instant) -> Vec<Riallineamento> {
    let mut fatti = Vec::new();
    let mut quando = adesso;
    while let Some(voce) = piano.prossima(quando) {
        piano.fatto(&voce.radice, true, quando);
        fatti.push(voce);
        quando += RESPIRO;
        assert!(fatti.len() < 20, "un giro senza fine: {fatti:?}");
    }
    fatti
}

fn voce(radice: &str, riarma: bool) -> Riallineamento {
    Riallineamento {
        radice: radice.to_owned(),
        riarma,
    }
}

#[test]
fn all_avvio_si_riallinea_dopo_il_risveglio_delle_cartelle() {
    let zero = Instant::now();
    let mut piano = Pianificatore::default();
    piano.imposta(NAS, Ritmo::Rete, zero);
    piano.imposta(CASA, Ritmo::Locale, zero);
    piano.imposta(r"\\nas\spenta", Ritmo::Nessuno, zero);
    piano.elenco_letto();
    assert_eq!(
        piano.prossima(zero),
        None,
        "il NAS non ha ancora finito di svegliarsi"
    );
    assert_eq!(piano.manca(zero), None);
    piano.avvio();
    assert_eq!(piano.manca(zero), Some(Duration::ZERO));
    assert_eq!(
        fai_tutti(&mut piano, zero),
        vec![voce(CASA, false), voce(NAS, false)],
        "una volta ciascuna, e non quella che non si guarda"
    );
    // Il risveglio può anche finire prima che l'elenco sia letto.
    let mut presto = Pianificatore::default();
    presto.avvio();
    presto.imposta(NAS, Ritmo::Rete, zero);
    presto.elenco_letto();
    assert_eq!(presto.prossima(zero), Some(voce(NAS, false)));
}

#[test]
fn una_cartella_aggiunta_dopo_l_avvio_non_si_riallinea_al_primo_armo() {
    let zero = Instant::now();
    let mut piano = piano_avviato(&[(CASA, Ritmo::Locale)], zero);
    fai_tutti(&mut piano, zero);
    // Chi l'aggiunge la scansiona: una lettura intera partita da sola gli
    // terrebbe il turno.
    piano.imposta(NAS, Ritmo::Rete, zero);
    assert_eq!(piano.prossima(zero + RESPIRO), None);
    // Il suo periodico però sì.
    assert_eq!(piano.prossima(zero + RIALLINEAMENTO), Some(voce(NAS, true)));
}

#[test]
fn un_salto_dell_orologio_conta_solo_sull_attesa() {
    let zero = Instant::now();
    let muro = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let minuto = Duration::from_secs(60);
    let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (CASA, Ritmo::Locale)], zero);
    fai_tutti(&mut piano, zero);

    // Un'attesa di un minuto che dura un minuto.
    piano.dormo(minuto, muro);
    assert!(!piano.sveglio(muro + minuto));
    // Poi una scansione di dieci minuti, che il muro vede passare ma che
    // non sta dentro nessuna attesa: la misura ricomincia dopo.
    let dopo_la_scansione = muro + minuto * 11;
    piano.dormo(minuto, dopo_la_scansione);
    assert!(
        !piano.sveglio(dopo_la_scansione + minuto),
        "una scansione lunga non è un sonno"
    );
    assert_eq!(piano.prossima(zero + RESPIRO * 2), None);
    // Fino al margine non è un sonno; oltre sì.
    piano.dormo(minuto, muro);
    assert!(!piano.sveglio(muro + minuto * 2 + SONNO_MARGINE));
    // Svegliarsi senza essersi addormentati non misura niente.
    assert!(!piano.sveglio(muro + minuto * 1_000));
    // Un orologio portato indietro non è un sonno.
    piano.dormo(minuto, muro);
    assert!(!piano.sveglio(muro - minuto * 60));

    // Un minuto d'attesa che sul muro dura due ore: il computer ha dormito.
    piano.dormo(minuto, muro);
    assert!(piano.sveglio(muro + minuto * 120));
    assert_eq!(
        fai_tutti(&mut piano, zero + RESPIRO * 2),
        vec![voce(CASA, true), voce(NAS, true)],
        "tutte riallineate, e ogni osservatore rifatto prima — anche il locale"
    );
}

#[test]
fn una_cartella_di_rete_tornata_si_riarma_e_si_riallinea() {
    let zero = Instant::now();
    let mut piano = piano_avviato(
        &[
            (NAS, Ritmo::Rete),
            (CASA, Ritmo::Locale),
            (r"\\nas\ferma", Ritmo::Nessuno),
        ],
        zero,
    );
    fai_tutti(&mut piano, zero);
    let poi = zero + RESPIRO * 2;
    piano.tornata(NAS);
    piano.tornata(r"\\nas\ferma");
    piano.tornata(r"\\nas\sconosciuta");
    assert_eq!(piano.prossima(poi), Some(voce(NAS, true)));
    // Il riarmo è fatto: resta la camminata, senza un secondo riarmo.
    piano.imposta(NAS, Ritmo::Rete, poi);
    assert_eq!(piano.prossima(poi), Some(voce(NAS, false)));
    piano.fatto(NAS, true, poi);
    assert_eq!(piano.prossima(poi + RESPIRO), None);
}

#[test]
fn le_cartelle_di_rete_si_riallineano_ogni_mezz_ora_riarmando_prima() {
    let zero = Instant::now();
    let mut piano = piano_avviato(&[(NAS, Ritmo::Rete)], zero);
    fai_tutti(&mut piano, zero);
    let un_attimo = Duration::from_secs(1);
    assert_eq!(piano.prossima(zero + RIALLINEAMENTO - un_attimo), None);
    assert_eq!(
        piano.manca(zero + RESPIRO * 2),
        Some(RIALLINEAMENTO - RESPIRO * 2)
    );
    let mezz_ora = zero + RIALLINEAMENTO;
    assert_eq!(piano.prossima(mezz_ora), Some(voce(NAS, true)));
    // Il turno è occupato: niente per un respiro, e niente riarmo doppio.
    piano.imposta(NAS, Ritmo::Rete, mezz_ora);
    piano.aspetta_un_respiro(mezz_ora);
    assert_eq!(piano.prossima(mezz_ora), None);
    assert_eq!(piano.prossima(mezz_ora + RESPIRO), Some(voce(NAS, false)));
    piano.fatto(NAS, true, mezz_ora + RESPIRO);
    assert_eq!(
        piano.prossima(mezz_ora + RESPIRO + RIALLINEAMENTO),
        Some(voce(NAS, true)),
        "e di nuovo fra mezz'ora"
    );
}

#[test]
fn anche_le_cartelle_locali_hanno_una_rete_di_sicurezza() {
    // `notify` 8.2 non dice quando un osservatore locale muore — il buffer
    // che trabocca durante una copia enorme — e fino a qui il disco di
    // casa si riallineava solo all'avvio: un'ora, e l'osservatore rifatto.
    let zero = Instant::now();
    let un_attimo = Duration::from_secs(1);
    let mut casa = piano_avviato(&[(CASA, Ritmo::Locale)], zero);
    assert_eq!(fai_tutti(&mut casa, zero), vec![voce(CASA, false)]);
    assert_eq!(
        casa.manca(zero + RESPIRO),
        Some(RIALLINEAMENTO_LOCALE - RESPIRO)
    );
    assert_eq!(
        casa.prossima(zero + RIALLINEAMENTO_LOCALE - un_attimo),
        None
    );
    let un_ora = zero + RIALLINEAMENTO_LOCALE;
    assert_eq!(casa.prossima(un_ora), Some(voce(CASA, true)));
    casa.imposta(CASA, Ritmo::Locale, un_ora);
    casa.fatto(CASA, true, un_ora);
    assert_eq!(casa.prossima(un_ora + RESPIRO), None);
    // Al risveglio dal sospeso si rifà anche l'osservatore locale.
    let muro = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let minuto = Duration::from_secs(60);
    casa.dormo(minuto, muro);
    assert!(casa.sveglio(muro + minuto * 120));
    assert_eq!(casa.prossima(un_ora + RESPIRO), Some(voce(CASA, true)));
    // Quella col solo riallineamento non ha un osservatore da rifare: lo
    // ritenta già ogni controllo.
    let mut senza = piano_avviato(&[(NAS, Ritmo::SoloRiallineamento)], zero);
    fai_tutti(&mut senza, zero);
    senza.dormo(minuto, muro);
    assert!(senza.sveglio(muro + minuto * 120));
    assert_eq!(senza.prossima(zero + RESPIRO), Some(voce(NAS, false)));
}

#[test]
fn il_solo_riallineamento_va_ogni_cinque_minuti() {
    let zero = Instant::now();
    let mut piano = piano_avviato(&[(NAS, Ritmo::SoloRiallineamento)], zero);
    assert_eq!(fai_tutti(&mut piano, zero), vec![voce(NAS, false)]);
    assert_eq!(
        piano.prossima(zero + SONDAGGIO - Duration::from_secs(1)),
        None
    );
    assert_eq!(
        piano.prossima(zero + SONDAGGIO),
        Some(voce(NAS, false)),
        "senza osservatore non c'è niente da riarmare"
    );
}

#[test]
fn piu_cause_insieme_fanno_un_riallineamento_solo() {
    let zero = Instant::now();
    let muro = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    // L'avvio, mai fatto; il sospeso; il ritorno; il cambio di passo; il
    // periodico scaduto: tutto insieme.
    let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (CASA, Ritmo::Locale)], zero);
    piano.dormo(Duration::from_secs(60), muro);
    assert!(piano.sveglio(muro + Duration::from_secs(3 * 60 * 60)));
    piano.tornata(NAS);
    piano.imposta(CASA, Ritmo::Nessuno, zero);
    piano.imposta(CASA, Ritmo::Locale, zero);
    let tardi = zero + RIALLINEAMENTO_LOCALE;
    assert_eq!(
        fai_tutti(&mut piano, tardi),
        vec![voce(CASA, true), voce(NAS, true)]
    );
    assert_eq!(piano.prossima(tardi + RESPIRO * 3), None);
}

#[test]
fn una_cartella_che_non_risponde_si_riprova_ma_non_in_un_ciclo_stretto() {
    let zero = Instant::now();
    let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (USB, Ritmo::Locale)], zero);
    assert_eq!(piano.prossima(zero), Some(voce(USB, false)));
    piano.fatto(USB, false, zero);
    assert_eq!(piano.prossima(zero + RESPIRO), Some(voce(NAS, false)));
    piano.fatto(NAS, false, zero + RESPIRO);
    let un_attimo = Duration::from_secs(1);
    assert_eq!(
        piano.prossima(zero + SONDAGGIO - un_attimo),
        None,
        "non subito: il disco è probabilmente ancora spento"
    );
    // Al più fra cinque minuti, anche il disco locale che di periodico ne
    // ha uno di un'ora; tutti e due con l'osservatore rifatto.
    assert_eq!(
        fai_tutti(&mut piano, zero + SONDAGGIO + RESPIRO),
        vec![voce(USB, true), voce(NAS, true)]
    );
    // Risposto, si torna al passo di sempre.
    let dopo = zero + SONDAGGIO * 2;
    assert_eq!(piano.prossima(dopo), None);
    assert_eq!(piano.prossima(zero + RIALLINEAMENTO), None);
    assert_eq!(
        piano.prossima(zero + SONDAGGIO + RESPIRO * 2 + RIALLINEAMENTO),
        Some(voce(NAS, true))
    );
}

#[test]
fn col_turno_occupato_e_dopo_ogni_scansione_si_lascia_un_respiro() {
    let zero = Instant::now();
    let mut piano = piano_avviato(&[(NAS, Ritmo::Rete), (CASA, Ritmo::Locale)], zero);
    let primo = piano.prossima(zero).expect("il primo");
    // Il turno è di «Scansiona»: si riprova dopo un respiro, non al
    // prossimo decimo di secondo.
    piano.aspetta_un_respiro(zero);
    assert!(!piano.libero(zero));
    assert_eq!(piano.manca(zero), Some(RESPIRO));
    assert_eq!(piano.prossima(zero + RESPIRO / 2), None);
    assert_eq!(piano.prossima(zero + RESPIRO), Some(primo.clone()));
    // E un respiro anche dopo una scansione, prima della prossima.
    piano.fatto(&primo.radice, true, zero + RESPIRO);
    assert_eq!(piano.prossima(zero + RESPIRO), None);
    assert_eq!(piano.fra_quanto_libero(zero + RESPIRO), RESPIRO);
    assert!(piano.prossima(zero + RESPIRO * 2).is_some());
}

#[test]
fn perdere_una_cartella_e_ritrovarla_non_perde_la_promessa() {
    let zero = Instant::now();
    let poi = |minuti: u64| zero + Duration::from_secs(60 * minuti);
    // Il disco USB: armato, staccato, e per due controlli «non si sa dove
    // sta» — la strada che fino alla 3.0.1 azzerava la promessa.
    let mut piano = piano_avviato(&[(USB, Ritmo::Locale)], zero);
    fai_tutti(&mut piano, zero);
    piano.imposta(USB, Ritmo::Nessuno, poi(1));
    piano.imposta(USB, Ritmo::Nessuno, poi(2));
    piano.imposta(USB, Ritmo::Nessuno, poi(3));
    assert_eq!(piano.prossima(poi(3)), None, "finché non c'è, niente");
    piano.imposta(USB, Ritmo::Locale, poi(4));
    assert_eq!(
        piano.prossima(poi(4)),
        Some(voce(USB, false)),
        "riattaccato, si riguarda"
    );
    // Il NAS senza avvisi: ripiega sul solo riallineamento, riprova
    // l'osservatore a ogni controllo senza farcela — niente di nuovo — e
    // quando ce la fa si riallinea.
    let mut piano = piano_avviato(&[(NAS, Ritmo::Rete)], zero);
    fai_tutti(&mut piano, zero);
    piano.imposta(NAS, Ritmo::SoloRiallineamento, poi(1));
    assert_eq!(piano.prossima(poi(1)), Some(voce(NAS, false)));
    piano.fatto(NAS, true, poi(1));
    piano.imposta(NAS, Ritmo::SoloRiallineamento, poi(2));
    piano.imposta(NAS, Ritmo::SoloRiallineamento, poi(3));
    assert_eq!(piano.prossima(poi(3)), None);
    piano.imposta(NAS, Ritmo::Rete, poi(4));
    assert_eq!(piano.prossima(poi(4)), Some(voce(NAS, false)));
}

#[test]
fn un_osservatore_vero_avvisa_e_spento_tace() {
    // Il sistema vero, su una cartella temporanea: l'unica prova che
    // aspetta il tempo reale, e al più un paio di secondi.
    let cartella = tempfile::tempdir().expect("cartella");
    let radice = cartella.path().to_string_lossy().into_owned();
    let (posta, orecchio) = channel();
    let osservatore = osserva(&radice, &posta).expect("osservatore");
    std::fs::create_dir(cartella.path().join("Album")).expect("cartella nuova");
    std::fs::write(cartella.path().join("Album").join("01.flac"), b"x").expect("file");
    let mut visti: Vec<Cambiamento> = Vec::new();
    let fine = Instant::now() + Duration::from_secs(3);
    while Instant::now() < fine && !visti.iter().any(|c| c.percorso.ends_with("01.flac")) {
        if let Ok(Messaggio::Avvisi(cambiati)) = orecchio.recv_timeout(Duration::from_millis(200)) {
            visti.extend(cambiati);
        }
    }
    assert!(
        visti.iter().any(|c| c.percorso.ends_with("01.flac")),
        "il file nuovo non è stato annunciato: {visti:?}"
    );
    // Quel che resta della raffica: la cartella cambiata dal file nuovo,
    // se il sistema la annuncia, non deve contare.
    while let Ok(Messaggio::Avvisi(cambiati)) = orecchio.recv_timeout(Duration::from_millis(300)) {
        visti.extend(cambiati);
    }
    let dell_album: Vec<&Cambiamento> = visti
        .iter()
        .filter(|c| c.percorso.ends_with("Album"))
        .collect();
    assert!(
        !dell_album.is_empty() && dell_album.iter().all(|c| c.nuovo),
        "la cartella creata è nuova, e la sua modifica non conta: {visti:?}"
    );
    // E fino in fondo: un album copiato è una scansione dell'album. Se la
    // modifica della cartella passasse, qui ci sarebbe la radice; se la
    // grafia dei percorsi del sistema non fosse quella della radice, niente.
    assert_eq!(
        cartelle_da_guardare(
            visti.clone(),
            vec![radice.clone()],
            PathRules::for_current_platform(),
            natura_di
        ),
        vec![cartella.path().join("Album").to_string_lossy().into_owned()]
    );
    // Spegnere è lasciar cadere l'osservatore: da lì in poi niente.
    drop(osservatore);
    while orecchio.recv_timeout(Duration::from_millis(100)).is_ok() {}
    std::fs::write(cartella.path().join("Album").join("02.flac"), b"y").expect("file");
    assert!(
        orecchio.recv_timeout(Duration::from_millis(500)).is_err(),
        "un osservatore spento ha parlato"
    );
}

#[test]
fn la_coda_dei_download_aspetta_il_turno_invece_di_saltarlo() {
    let dati = tempfile::tempdir().expect("cartella dati di prova");
    let stato = Stato::apri(dati.path().to_path_buf());
    let preso = stato.turno_di_scansione().expect("il primo turno");
    std::thread::scope(|filo| {
        let attesa = filo.spawn(|| {
            let inizio = Instant::now();
            let turno = aspetta_il_turno(&stato, &|| false);
            (turno.is_some(), inizio.elapsed())
        });
        std::thread::sleep(Duration::from_millis(700));
        drop(preso);
        let (preso_dopo, quanto) = attesa.join().expect("il filo che aspetta");
        assert!(preso_dopo, "il turno doveva arrivare");
        assert!(
            quanto >= Duration::from_millis(600),
            "doveva aspettare la fine dell'altra: {quanto:?}"
        );
    });
    // E chi deve smettere di aspettare smette.
    let _ancora = stato.turno_di_scansione().expect("di nuovo libero");
    assert!(aspetta_il_turno(&stato, &|| true).is_none());
}

// ── il motore, dopo le passate ──

fn tutte(cartelle: &[&str]) -> Vec<String> {
    cartelle
        .iter()
        .map(|cartella| (*cartella).to_owned())
        .collect()
}

/// Una passata arrivata in fondo: `saltate` non hanno risposto, `vuote`
/// hanno risposto vuote (e stanno anche fra le saltate, come nel
/// rapporto), `persi` non si sono letti.
fn passata(saltate: &[&str], vuote: &[&str], persi: &[&str]) -> Passata {
    let mut tutte_le_saltate = tutte(saltate);
    tutte_le_saltate.extend(tutte(vuote));
    Passata::Fatta {
        saltate: tutte_le_saltate,
        vuote: tutte(vuote),
        rami_persi: tutte(persi),
        fermata: false,
    }
}

/// Un motore che ha letto l'elenco e finito il risveglio, col prossimo
/// controllo fuori dalla prova.
fn motore_con(radici: &[(&str, Ritmo)], adesso: Instant) -> Motore {
    let mut motore = Motore::nuovo(adesso);
    motore.elenco = radici
        .iter()
        .map(|(radice, _)| (*radice).to_owned())
        .collect();
    motore.piano = piano_avviato(radici, adesso);
    motore.configurata = true;
    motore.prossimo_controllo = adesso + Duration::from_secs(7 * 24 * 60 * 60);
    motore
}

#[test]
fn un_nas_spento_non_trattiene_le_cartelle_di_casa() {
    // La simulazione della revisione: con un rinvio solo per tutti, il NAS
    // spento — riallineato ogni cinque minuti, e ogni volta saltato — si
    // portava dietro l'album rinviato sul disco di casa. In ventiquattro
    // ore, 286 riallineamenti del NAS e nessuna scansione di casa.
    let zero = Instant::now();
    let regole = PathRules::for_current_platform();
    let mut motore = motore_con(
        &[(CASA, Ritmo::Locale), (NAS, Ritmo::SoloRiallineamento)],
        zero,
    );
    // I riallineamenti dell'avvio: casa risponde, il NAS no; e un album
    // sul NAS, saltato, aspetta la sua radice.
    motore.dopo_il_riallineamento(CASA, Vec::new(), passata(&[], &[], &[]), zero);
    motore.dopo_il_riallineamento(NAS, Vec::new(), passata(&[NAS], &[], &[]), zero);
    let sul_nas = format!(r"{NAS}\Album");
    motore.dopo_la_riscansione(
        std::slice::from_ref(&sul_nas),
        passata(&[&sul_nas], &[], &[]),
        zero,
    );
    // Un minuto dopo, un album copiato a casa trova il disco esterno che
    // girava ancora a vuoto.
    let minuto = zero + Duration::from_secs(60);
    let a_casa = format!(r"{CASA}\Album");
    motore.dopo_la_riscansione(
        std::slice::from_ref(&a_casa),
        passata(&[&a_casa], &[], &[]),
        minuto,
    );
    // Il filo, come `lavora`: prima il riallineamento, poi quel che è
    // pronto. Ogni passata sul NAS dura tre secondi e lo trova spento.
    let tre_secondi = Duration::from_secs(3);
    let mut adesso = minuto;
    let mut del_nas = 0_u32;
    let ripresa = loop {
        adesso += prossima_attesa(&motore, adesso);
        assert!(
            adesso < zero + Duration::from_secs(24 * 60 * 60),
            "il disco di casa non si è più riguardato ({del_nas} riallineamenti del NAS)"
        );
        if let Some(voce) = motore.piano.prossima(adesso) {
            assert_eq!(voce.radice, NAS);
            del_nas += 1;
            let assorbiti = motore.attesa.togli_sotto(NAS, regole);
            adesso += tre_secondi;
            motore.dopo_il_riallineamento(NAS, assorbiti, passata(&[NAS], &[], &[]), adesso);
            continue;
        }
        if motore.piano.fra_quanto_libero(adesso).is_zero() && motore.attesa.pronto(adesso) {
            let presi = motore.attesa.prendi(adesso);
            if presi.contains(&nuovo(CASA)) {
                break adesso;
            }
            let cartelle: Vec<String> = presi.into_iter().map(|c| c.percorso).collect();
            let mute: Vec<&str> = cartelle.iter().map(String::as_str).collect();
            adesso += tre_secondi;
            motore.dopo_la_riscansione(&cartelle, passata(&mute, &[], &[]), adesso);
        }
    };
    assert_eq!(
        ripresa,
        minuto + SONDAGGIO,
        "al suo rinvio, non a quello del NAS"
    );
    assert_eq!(del_nas, 1, "intanto il NAS si è riprovato una volta sola");
}

#[test]
fn una_cartella_svuotata_davvero_resta_al_suo_passo() {
    let zero = Instant::now();
    let album = nuovo(r"\\nas\musica\Album");
    // Risponde vuota: la guardia per radice non ha tolto niente. Non è «non
    // risponde»: riprovarla ogni cinque minuti era un riallineamento ogni
    // cinque minuti per sempre, su una cartella svuotata davvero.
    let mut motore = motore_con(&[(NAS, Ritmo::Rete)], zero);
    assert_eq!(motore.piano.prossima(zero), Some(voce(NAS, false)));
    motore.dopo_il_riallineamento(NAS, vec![album.clone()], passata(&[], &[NAS], &[]), zero);
    assert_eq!(
        motore.attesa.manca(zero),
        None,
        "quel che aspettava sotto lo guarda il suo prossimo riallineamento"
    );
    assert_eq!(motore.piano.prossima(zero + SONDAGGIO), None);
    assert_eq!(
        motore.piano.manca(zero + RESPIRO),
        Some(RIALLINEAMENTO - RESPIRO)
    );
    assert_eq!(
        motore.piano.prossima(zero + RIALLINEAMENTO),
        Some(voce(NAS, true)),
        "al suo giro"
    );
    // Una che non risponde invece si riprova fra cinque minuti, e con lei
    // quel che aspettava.
    let mut muta = motore_con(&[(NAS, Ritmo::Rete)], zero);
    muta.dopo_il_riallineamento(NAS, vec![album], passata(&[NAS], &[], &[]), zero);
    assert_eq!(muta.attesa.manca(zero), Some(SONDAGGIO));
    assert_eq!(muta.piano.prossima(zero + SONDAGGIO), Some(voce(NAS, true)));
    // In una scansione di più cartelle: la vuota non torna, la muta sì.
    let altra = r"\\nas\altra";
    let mut motore = motore_con(&[(NAS, Ritmo::Rete), (altra, Ritmo::Rete)], zero);
    motore.dopo_la_riscansione(&tutte(&[NAS, altra]), passata(&[altra], &[NAS], &[]), zero);
    assert_eq!(motore.attesa.manca(zero), Some(SONDAGGIO));
    assert_eq!(motore.attesa.prendi(zero + SONDAGGIO), vec![nuovo(altra)]);
    // E un ramo perso sotto la vuota non si riguarda da solo: una
    // sottocartella non ha la guardia per radice, e la giudicherebbe a
    // pezzi.
    let ramo = format!(r"{NAS}\System Volume Information");
    let mut motore = motore_con(&[(NAS, Ritmo::Rete)], zero);
    motore.dopo_il_riallineamento(NAS, Vec::new(), passata(&[], &[NAS], &[&ramo]), zero);
    assert_eq!(motore.attesa.manca(zero), None);
    motore.dopo_la_riscansione(&tutte(&[NAS]), passata(&[], &[NAS], &[&ramo]), zero);
    assert_eq!(motore.attesa.manca(zero), None);
}

#[test]
fn col_turno_occupato_non_si_chiede_niente_al_disco() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DOMANDE: AtomicUsize = AtomicUsize::new(0);
    fn natura_contata(_: &Path) -> Natura {
        DOMANDE.fetch_add(1, Ordering::SeqCst);
        Natura::Cartella
    }
    let dati = tempfile::tempdir().expect("cartella dati di prova");
    let stato = Stato::apri(dati.path().to_path_buf());
    let zero = Instant::now();
    let mut motore = motore_con(&[(CASA, Ritmo::Locale)], zero);
    let brano = format!(r"{CASA}\Album\01.flac");
    motore.attesa.avvisati([vecchio(&brano)], zero);
    let pronto = zero + ACCORPAMENTO;
    // «Scansiona» ha il turno: si riprova ogni respiro, e ogni volta solo il
    // turno — prima, ogni due secondi, una domanda al disco per cartella.
    let di_scansiona = stato.turno_di_scansione().expect("il turno di «Scansiona»");
    let mut quando = pronto;
    for _ in 0..5 {
        assert!(
            motore
                .prepara_la_riscansione(&stato, quando, natura_contata)
                .is_none()
        );
        assert_eq!(motore.piano.fra_quanto_libero(quando), RESPIRO);
        quando += RESPIRO;
    }
    assert_eq!(
        DOMANDE.load(Ordering::SeqCst),
        0,
        "il disco è stato interrogato"
    );
    assert!(motore.attesa.pronto(quando), "e niente è andato perso");
    drop(di_scansiona);
    let (turno, cartelle) = motore
        .prepara_la_riscansione(&stato, quando, natura_contata)
        .expect("il turno libero");
    assert_eq!(cartelle, vec![format!(r"{CASA}\Album")]);
    assert!(DOMANDE.load(Ordering::SeqCst) > 0);
    assert!(!motore.attesa.pronto(quando), "preso");
    drop(turno);
}

#[test]
fn un_ramo_perso_si_riguarda_fra_poco_ma_non_per_sempre() {
    let regole = PathRules {
        case_insensitive: true,
    };
    let zero = Instant::now();
    let ramo = r"C:\Musica\CD nuovo";
    let casa = tutte(&[CASA]);
    let il_ramo = tutte(&[ramo]);
    let mut attesa = Accorpatore::default();
    // Il riallineamento di casa non ha letto il `.cue` del CD appena
    // copiato: lo si riguarda fra un minuto, non al giro dopo.
    assert!(attesa.rami(&casa, &il_ramo, zero, regole).is_empty());
    assert_eq!(attesa.manca(zero), Some(RAMO_PERSO));
    // Una cartella a permessi negati non si legge mai: ogni volta il
    // doppio, e poi basta.
    let mut adesso = zero;
    let mut attese = Vec::new();
    let mut lasciati = Vec::new();
    for _ in 0..10 {
        let Some(manca) = attesa.manca(adesso) else {
            break;
        };
        attese.push(manca);
        adesso += manca;
        assert_eq!(attesa.prendi(adesso), vec![nuovo(ramo)]);
        lasciati.extend(attesa.rami(&il_ramo, &il_ramo, adesso, regole));
    }
    let minuti = |quanti: u64| Duration::from_secs(60 * quanti);
    assert_eq!(
        attese,
        vec![minuti(1), minuti(2), minuti(4), minuti(8), minuti(16)]
    );
    assert_eq!(lasciati, il_ramo, "detto una volta sola");
    // Il riallineamento di casa lo riperde: niente giro da capo.
    assert!(attesa.rami(&casa, &il_ramo, adesso, regole).is_empty());
    assert_eq!(attesa.manca(adesso), None);
    // Una passata che non l'ha guardato — la radice non ha risposto — non
    // lo rimette in regola.
    assert!(attesa.rami(&[], &[], adesso, regole).is_empty());
    attesa.rami(&casa, &il_ramo, adesso, regole);
    assert_eq!(attesa.manca(adesso), None);
    // Una che lo legge sì: la prossima volta si riprova fra un minuto.
    attesa.rami(&casa, &[], adesso, regole);
    attesa.rami(&casa, &il_ramo, adesso, regole);
    assert_eq!(attesa.manca(adesso), Some(RAMO_PERSO));

    // E dalla sorveglianza: l'album appena copiato, il cui `.cue` non si è
    // letto. Il resto della passata è andato.
    let mut motore = motore_con(&[(CASA, Ritmo::Locale)], zero);
    let album = format!(r"{CASA}\CD nuovo");
    motore.dopo_la_riscansione(
        std::slice::from_ref(&album),
        passata(&[], &[], &[&album]),
        zero,
    );
    assert_eq!(motore.attesa.manca(zero), Some(RAMO_PERSO));
    assert_eq!(motore.attesa.prendi(zero + RAMO_PERSO), vec![nuovo(&album)]);
    // E dal riallineamento.
    motore.dopo_il_riallineamento(CASA, Vec::new(), passata(&[], &[], &[&album]), zero);
    assert_eq!(motore.attesa.manca(zero), Some(RAMO_PERSO * 2));
}

#[test]
fn la_cartella_scelta_al_primo_avvio_non_si_riallinea_da_sola() {
    let zero = Instant::now();
    // Il primo avvio: all'apertura l'elenco è vuoto, nei primi secondi se
    // ne sceglie una, e la si scansiona a mano. Arriva col primo elenco
    // letto, ma non c'era quando il filo è partito.
    let mut piano = Pianificatore::default();
    piano.fotografa(Vec::new());
    piano.imposta(CASA, Ritmo::Locale, zero);
    piano.elenco_letto();
    piano.avvio();
    assert_eq!(piano.prossima(zero), None, "la scansiona chi l'ha scelta");
    assert_eq!(
        piano.prossima(zero + RIALLINEAMENTO_LOCALE),
        Some(voce(CASA, true)),
        "il suo periodico sì"
    );
    // Quelle che c'erano all'apertura si riallineano, e solo loro.
    let mut piano = Pianificatore::default();
    piano.fotografa(tutte(&[NAS]));
    piano.imposta(NAS, Ritmo::Rete, zero);
    piano.imposta(CASA, Ritmo::Locale, zero);
    piano.elenco_letto();
    piano.avvio();
    assert_eq!(fai_tutti(&mut piano, zero), vec![voce(NAS, false)]);
    // Una dell'apertura tolta e rimessa dopo è un'aggiunta come le altre.
    piano.tieni_solo(&tutte(&[CASA]));
    piano.imposta(NAS, Ritmo::Rete, zero + RESPIRO * 2);
    assert_eq!(piano.prossima(zero + RESPIRO * 3), None);
}

#[test]
fn a_sorveglianza_spenta_niente_si_riprova_da_se() {
    let zero = Instant::now();
    let album = format!(r"{NAS}\Album");
    let l_album = std::slice::from_ref(&album);
    let mut motore = motore_con(&[(NAS, Ritmo::Rete)], zero);
    motore.dopo_la_riscansione(l_album, passata(&[&album], &[], &[]), zero);
    assert_eq!(motore.attesa.manca(zero), Some(SONDAGGIO));
    // Spenta: quel che aspettava il rinvio cade. Prima girava ogni cinque
    // minuti per tutta la sessione.
    assert!(motore.imposta_modo(Modo::Spenta));
    assert_eq!(motore.attesa.manca(zero), None);
    assert!(!motore.imposta_modo(Modo::Spenta));
    // Una richiesta di riguardare vale anche da spenta...
    motore.attesa.da_riguardare([album.clone()], zero);
    let pronto = zero + ACCORPAMENTO;
    assert_eq!(motore.attesa.prendi(pronto), vec![nuovo(&album)]);
    // ...ma se non va, non si riprova da sé: né il disco che non risponde,
    // né la scansione fallita, né la sottocartella che non si legge.
    motore.dopo_la_riscansione(l_album, passata(&[&album], &[], &[]), pronto);
    motore.dopo_la_riscansione(l_album, Passata::Fallita, pronto);
    motore.dopo_la_riscansione(l_album, passata(&[], &[], &[&album]), pronto);
    motore.dopo_il_riallineamento(NAS, vec![nuovo(&album)], passata(&[NAS], &[], &[]), pronto);
    assert_eq!(motore.attesa.manca(pronto), None);
    // Riaccesa, si torna a riprovare.
    assert!(motore.imposta_modo(Modo::Tutte));
    motore.dopo_la_riscansione(l_album, passata(&[&album], &[], &[]), pronto);
    assert_eq!(motore.attesa.manca(pronto), Some(SONDAGGIO));
}
