//! I fogli `.cue` di una libreria vera, letti come li leggerà la scansione.
//!
//! Cammina le radici, legge ogni `.cue`, cerca l'immagine fra i nomi della sua
//! cartella e racconta: quanti fogli, quanti descrivono un'immagine, con quale
//! regola si è trovata, perché gli altri si ignorano, quanti brani virtuali ne
//! escono e di che formato sono le immagini. Poi passa tutto da
//! `espandi_immagini`, che deve dare lo stesso numero di abbinamenti — la
//! differenza sono le immagini reclamate da due fogli, ed è elencata.
//!
//!     cargo run -p rymembra-domain --example fogli -- "\\nas\Cd;\\nas\Altro"
//!
//! Più radici si separano con `;`. **Solo lettura**: `read_dir`, `metadata` e
//! `read`, nient'altro tocca le radici. Con un secondo argomento, l'elenco
//! delle immagini abbinate (`immagine<TAB>cue<TAB>brani`) si scrive in quel
//! file locale: serve a confrontarlo con un database.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stdout)]

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};
use rymembra_domain::codifica::leggi_byte;
use rymembra_domain::cue::{
    Foglio, FoglioTrovato, brano_d_immagine, espandi_immagini, leggi_foglio, trova_immagine,
};
use rymembra_domain::paths::{
    PathRules, base_name, extension_of, file_stem, is_supported_audio_path,
};
use rymembra_domain::scan_plan::DiscoveredFile;

/// Una cartella con i suoi file (non le sottocartelle).
struct Cartella {
    file: Vec<DiscoveredFile>,
}

fn cammina(radice: &Path, cartelle: &mut Vec<Cartella>, errori: &mut Vec<String>) {
    let mut pila = vec![radice.to_path_buf()];
    while let Some(dir) = pila.pop() {
        let voci = match fs::read_dir(&dir) {
            Ok(voci) => voci,
            Err(e) => {
                errori.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        let mut file = Vec::new();
        for voce in voci.flatten() {
            let Ok(tipo) = voce.file_type() else {
                continue;
            };
            if tipo.is_symlink() {
                continue;
            }
            let percorso = voce.path();
            if tipo.is_dir() {
                pila.push(percorso);
                continue;
            }
            let Ok(meta) = voce.metadata() else {
                continue;
            };
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0);
            file.push(DiscoveredFile {
                path: percorso.to_string_lossy().into_owned(),
                size_bytes: meta.len(),
                modified_ms,
            });
        }
        cartelle.push(Cartella { file });
    }
}

/// La codifica del foglio, con la stessa precedenza di `cue::leggi_foglio`.
fn codifica(byte: &[u8]) -> &'static str {
    match Encoding::for_bom(byte) {
        Some((enc, _)) if enc == UTF_8 => "UTF-8 con BOM",
        Some((enc, _)) => enc.name(),
        None if std::str::from_utf8(byte).is_ok() => {
            if byte.contains(&0) {
                "UTF-8 valido ma con NUL (UTF-16 senza BOM?)"
            } else {
                "UTF-8 senza BOM"
            }
        }
        None => "windows-1252",
    }
}

fn decodifica(byte: &[u8]) -> Cow<'_, str> {
    if let Some((enc, bom)) = Encoding::for_bom(byte) {
        return enc
            .decode_without_bom_handling(byte.get(bom..).unwrap_or_default())
            .0;
    }
    match std::str::from_utf8(byte) {
        Ok(t) => Cow::Borrowed(t),
        Err(_) => WINDOWS_1252.decode_without_bom_handling(byte).0,
    }
}

/// Perché `leggi_foglio` ha detto `None`: una diagnosi grossolana, per contare.
fn motivo(byte: &[u8]) -> &'static str {
    let testo = decodifica(byte);
    let mut nomi: Vec<String> = Vec::new();
    let mut tracce = 0_usize;
    for riga in testo.split(['\r', '\n']) {
        let riga = riga.trim();
        let parola = riga.split_whitespace().next().unwrap_or("");
        if parola.eq_ignore_ascii_case("FILE") {
            nomi.push(riga.get(4..).unwrap_or("").trim().to_lowercase());
        } else if parola.eq_ignore_ascii_case("TRACK") {
            tracce += 1;
        }
    }
    if nomi.is_empty() {
        "nessun FILE"
    } else if nomi.len() > 1 {
        nomi.sort();
        nomi.dedup();
        if nomi.len() == 1 {
            "più FILE, tutti con lo stesso nome"
        } else {
            "più FILE (brani separati)"
        }
    } else if tracce < 2 {
        "meno di 2 TRACK"
    } else {
        "altro: INDEX 01, tempi o numeri di traccia"
    }
}

/// Con quale regola ha vinto `scelto`, e quanti candidati la stessa regola
/// avrebbe accettato.
fn regola_di(nome_cue: &str, foglio: &Foglio, scelto: &str, nomi: &[&str]) -> (usize, usize) {
    let scritto = base_name(&foglio.file).to_lowercase();
    let radice_file = file_stem(&scritto).to_owned();
    let radice_cue = file_stem(nome_cue).to_lowercase();
    let audio = nomi.iter().filter(|n| is_supported_audio_path(n));
    let scelto = scelto.to_lowercase();
    if scelto == scritto {
        (1, audio.filter(|n| n.to_lowercase() == scritto).count())
    } else if file_stem(&scelto) == radice_file {
        (
            2,
            audio
                .filter(|n| file_stem(n).to_lowercase() == radice_file)
                .count(),
        )
    } else {
        (
            3,
            audio
                .filter(|n| file_stem(n).to_lowercase() == radice_cue)
                .count(),
        )
    }
}

/// Le estensioni della cartella, contate: `flac×14, jpg×2`.
fn estensioni(nomi: &[&str]) -> String {
    let mut conta: BTreeMap<String, usize> = BTreeMap::new();
    for nome in nomi {
        *conta.entry(extension_of(nome)).or_default() += 1;
    }
    conta
        .iter()
        .map(|(e, n)| format!("{}×{n}", if e.is_empty() { "-" } else { e }))
        .collect::<Vec<_>>()
        .join(", ")
}

fn main() {
    let argomento = std::env::args().nth(1).expect("le radici, separate da `;`");
    let radici: Vec<&str> = argomento
        .split(';')
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .collect();
    let rules = PathRules::for_current_platform();

    let inizio = Instant::now();
    let mut cartelle = Vec::new();
    let mut errori = Vec::new();
    for radice in &radici {
        cammina(Path::new(radice), &mut cartelle, &mut errori);
    }
    let file_totali: usize = cartelle.iter().map(|c| c.file.len()).sum();
    println!(
        "Camminata: {} cartelle, {file_totali} file, {} errori, {:.1} s",
        cartelle.len(),
        errori.len(),
        inizio.elapsed().as_secs_f64()
    );
    for e in &errori {
        println!("  errore: {e}");
    }

    let mut fogli = 0_usize;
    let mut illeggibili = Vec::new();
    let mut codifiche: BTreeMap<&str, usize> = BTreeMap::new();
    let mut ignorati: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut validi = 0_usize;
    let mut per_regola: BTreeMap<usize, usize> = BTreeMap::new();
    let mut per_estensione: BTreeMap<String, usize> = BTreeMap::new();
    let mut brani = 0_usize;
    let mut ambigui = Vec::new();
    let mut regola3 = Vec::new();
    let mut senza_immagine = Vec::new();
    let mut con_pregap_iniziale = 0_usize;
    let mut con_pregap_lungo: Vec<String> = Vec::new();
    let mut cp1252_altrimenti: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    // (cue, immagine, foglio) per ogni abbinamento trovato da `trova_immagine`.
    let mut abbinati_qui: Vec<(String, String, Foglio)> = Vec::new();
    let mut trovati_fogli: Vec<FoglioTrovato> = Vec::new();
    let mut tutti: Vec<DiscoveredFile> = Vec::with_capacity(file_totali);

    for cartella in &cartelle {
        let nomi: Vec<&str> = cartella.file.iter().map(|f| base_name(&f.path)).collect();
        for cue in &cartella.file {
            if extension_of(&cue.path) != "cue" {
                continue;
            }
            fogli += 1;
            let byte = match fs::read(&cue.path) {
                Ok(b) => b,
                Err(e) => {
                    illeggibili.push(format!("{}: {e}", cue.path));
                    continue;
                }
            };
            let quale = codifica(&byte);
            *codifiche.entry(quale).or_default() += 1;
            if quale == "windows-1252" {
                // Il foglio si legge in windows-1252 come dice il contratto; qui
                // si conta quanti, a giudizio di `codifica::leggi_byte`, erano
                // scritti in un'altra codifica a un byte (cirillico, …).
                let (_, altra) = leggi_byte(&byte);
                cp1252_altrimenti
                    .entry(altra)
                    .or_default()
                    .push(cue.path.clone());
            }
            let Some(foglio) = leggi_foglio(&byte) else {
                ignorati
                    .entry(motivo(&byte))
                    .or_default()
                    .push(cue.path.clone());
                continue;
            };
            validi += 1;
            if foglio.tracce.first().is_some_and(|t| t.inizio_cd > 0) {
                con_pregap_iniziale += 1;
            }
            // Oltre due secondi: lì può esserci musica (una traccia nascosta).
            if let Some(primo) = foglio.tracce.first().filter(|t| t.inizio_cd > 150) {
                con_pregap_lungo.push(format!("{} ({} fotogrammi)", cue.path, primo.inizio_cd));
            }
            let nome_cue = base_name(&cue.path);
            match trova_immagine(nome_cue, &foglio, &nomi) {
                Some(scelto) => {
                    let (regola, candidati) = regola_di(nome_cue, &foglio, scelto, &nomi);
                    *per_regola.entry(regola).or_default() += 1;
                    *per_estensione.entry(extension_of(scelto)).or_default() += 1;
                    brani += foglio.tracce.len();
                    let immagine = cartella
                        .file
                        .iter()
                        .find(|f| base_name(&f.path) == scelto)
                        .map(|f| f.path.clone())
                        .unwrap_or_default();
                    abbinati_qui.push((cue.path.clone(), immagine, foglio.clone()));
                    if candidati > 1 {
                        ambigui.push(format!(
                            "{} → {scelto} (regola {regola}, {candidati} candidati)",
                            cue.path
                        ));
                    }
                    if regola == 3 {
                        regola3.push(format!(
                            "{}\n      FILE \"{}\" → {scelto}\n      cartella: {}",
                            cue.path,
                            foglio.file,
                            estensioni(&nomi)
                        ));
                    }
                }
                None => {
                    let audio: Vec<&str> = nomi
                        .iter()
                        .copied()
                        .filter(|n| is_supported_audio_path(n))
                        .collect();
                    senza_immagine.push(format!(
                        "{}\n      FILE \"{}\" · {} brani · audio accanto: {} {:?}\n      cartella: {}",
                        cue.path,
                        foglio.file,
                        foglio.tracce.len(),
                        audio.len(),
                        audio.iter().take(3).collect::<Vec<_>>(),
                        estensioni(&nomi)
                    ));
                }
            }
            trovati_fogli.push(FoglioTrovato {
                cue: cue.clone(),
                foglio,
            });
        }
        tutti.extend(cartella.file.iter().cloned());
    }

    println!();
    println!("Fogli .cue: {fogli}");
    println!("  illeggibili: {}", illeggibili.len());
    for i in &illeggibili {
        println!("    {i}");
    }
    println!("  codifiche:");
    for (c, n) in &codifiche {
        println!("    {c}: {n}");
    }
    println!("  i windows-1252 secondo codifica::leggi_byte:");
    for (c, elenco) in &cp1252_altrimenti {
        println!("    {c}: {}", elenco.len());
        if *c != "windows-1252" {
            for p in elenco {
                println!("      {p}");
            }
        }
    }
    let quanti_ignorati: usize = ignorati.values().map(Vec::len).sum();
    println!("  validi come immagine: {validi}");
    println!("  ignorati: {quanti_ignorati}");
    for (m, elenco) in &ignorati {
        println!("    {m}: {}", elenco.len());
        if !m.starts_with("più FILE (") {
            for p in elenco {
                println!("      {p}");
            }
        }
    }

    let abbinati: usize = per_regola.values().sum();
    println!();
    println!("Immagini abbinate: {abbinati}");
    for (r, n) in &per_regola {
        println!("  regola {r}: {n}");
    }
    println!("  per estensione:");
    for (e, n) in &per_estensione {
        println!("    {e}: {n}");
    }
    println!("  brani virtuali: {brani}");
    println!(
        "  con il primo INDEX 01 dopo 00:00:00: {con_pregap_iniziale} (dopo 2 s: {})",
        con_pregap_lungo.len()
    );
    for p in &con_pregap_lungo {
        println!("    {p}");
    }
    println!("  più candidati per la regola vincente: {}", ambigui.len());
    for a in &ambigui {
        println!("    {a}");
    }
    println!("  abbinati con la regola 3:");
    for r in &regola3 {
        println!("    {r}");
    }
    println!();
    println!("Validi senza immagine: {}", senza_immagine.len());
    for s in &senza_immagine {
        println!("    {s}");
    }

    // La stessa cosa, da `espandi_immagini`: è quella che userà la scansione.
    // Come una libreria vuota: nessun `.cue` ha già dei brani, e fra due fogli
    // per la stessa immagine decide l'ordine di percorso.
    let prima = tutti.len();
    let espansione = espandi_immagini(tutti, trovati_fogli, &HashSet::new(), rules);
    let virtuali = espansione
        .trovati
        .iter()
        .filter(|f| brano_d_immagine(&f.path).is_some())
        .count();
    let mut chiavi = HashSet::with_capacity(espansione.trovati.len());
    let doppi = espansione
        .trovati
        .iter()
        .filter(|f| !chiavi.insert(rymembra_domain::paths::path_key(&f.path, rules)))
        .count();
    println!();
    println!(
        "espandi_immagini: {} abbinamenti, {virtuali} voci virtuali, file {prima} → {} ({doppi} percorsi doppi)",
        espansione.abbinamenti.len(),
        espansione.trovati.len()
    );
    let mut uniche: BTreeMap<String, usize> = BTreeMap::new();
    for a in &espansione.abbinamenti {
        *uniche.entry(extension_of(&a.immagine.path)).or_default() += 1;
    }
    println!("  immagini distinte per estensione:");
    for (e, n) in &uniche {
        println!("    {e}: {n}");
    }
    let presi: HashSet<&str> = espansione
        .abbinamenti
        .iter()
        .map(|a| a.cue.path.as_str())
        .collect();
    let vincitori: BTreeMap<&str, &Foglio> = espansione
        .abbinamenti
        .iter()
        .map(|a| (a.immagine.path.as_str(), &a.foglio))
        .collect();
    // Un'immagine reclamata da due fogli va al primo: si controlla che il
    // secondo dica la stessa cosa, a parte il nome scritto dopo FILE.
    let mut uguali = 0_usize;
    let mut diversi = Vec::new();
    for (cue, immagine, foglio) in abbinati_qui
        .iter()
        .filter(|(c, _, _)| !presi.contains(c.as_str()))
    {
        let Some(vincitore) = vincitori.get(immagine.as_str()) else {
            diversi.push(format!("{cue} (immagine {immagine} senza vincitore)"));
            continue;
        };
        let mut confronto = foglio.clone();
        confronto.file.clone_from(&vincitore.file);
        if confronto == **vincitore {
            uguali += 1;
        } else {
            diversi.push(format!("{cue} → {immagine}"));
        }
    }
    println!(
        "  immagini reclamate da un secondo foglio: {} (uguale al primo tranne FILE: {uguali}, diverso: {})",
        uguali + diversi.len(),
        diversi.len()
    );
    for d in &diversi {
        println!("    diverso: {d}");
    }
    if let Some(uscita) = std::env::args().nth(2) {
        let righe: String = espansione
            .abbinamenti
            .iter()
            .map(|a| {
                format!(
                    "{}	{}	{}
",
                    a.immagine.path,
                    a.cue.path,
                    a.foglio.tracce.len()
                )
            })
            .collect();
        fs::write(&uscita, righe).expect("l'elenco delle immagini");
        println!("Elenco delle immagini in {uscita}");
    }
    println!("Tempo totale: {:.1} s", inizio.elapsed().as_secs_f64());
}
