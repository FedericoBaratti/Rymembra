//! Dove va a finire, e come si chiama, un brano preso da un catalogo.
//!
//! Stava in `yt_match`, insieme alla scelta del video, e ci stava perché
//! all'epoca il posto da cui un brano arrivava era uno solo. Adesso le fonti
//! sono più d'una — l'Internet Archive, domani altre — e nessuna di loro ha
//! voce in capitolo su come si chiami il file: quello lo decide la libreria di
//! chi ascolta, non chi consegna i byte.
//!
//! # Perché è una decisione di dominio e non di chi scarica
//!
//! Perché sbagliarla si vede tardi e si paga caro: un nome con un carattere che
//! il filesystem non accetta fa fallire lo scaricamento **dopo** aver preso i
//! byte, e un nome che finisce con uno spazio su Windows si crea ma non si
//! riapre. Provare queste cose deve costare una chiamata di funzione, non una
//! rete e un disco.

use crate::esterno::BranoEsterno;
use crate::text::collapse_whitespace;

/// I caratteri che nessun filesystem che ci interessa accetta in un nome.
const VIETATI: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// I nomi che Windows riserva ai dispositivi, a qualunque estensione.
///
/// Una cartella chiamata `CON` non si può creare, e il guasto arriva come un
/// errore di permessi che manda a cercare nel posto sbagliato. Un artista
/// chiamato `AUX` o un album `NUL` sono rari ma esistono.
///
/// Stavano in `organize`, che scriveva cartelle a partire dai tag; con il
/// ritiro del riordino l'unico posto che costruisce ancora un percorso dai
/// metadati è questo, e la costante lo ha seguito.
const NOMI_RISERVATI: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Quanti caratteri al massimo può essere lungo un segmento di percorso.
///
/// Centoventi e non 255: i segmenti sono tre — interprete, album, file — e su
/// Windows il percorso completo ha comunque un tetto. Tagliare qui è meglio che
/// scoprire a metà scaricamento che il file non si può creare.
const LUNGHEZZA_MASSIMA: usize = 120;

/// Un titolo reso adatto a essere un nome di cartella o di file.
///
/// L'insieme dei caratteri vietati è scritto per esteso e **senza** classi
/// Unicode, così è lo stesso su ogni piattaforma e si legge senza eseguirlo.
#[must_use]
pub fn segmento_sicuro(grezzo: &str) -> String {
    let sostituito: String = grezzo
        .chars()
        .map(|c| {
            if VIETATI.contains(&c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let pulito: String = collapse_whitespace(&sostituito)
        .chars()
        .take(LUNGHEZZA_MASSIMA)
        .collect();
    // Spazi **e punti** in coda: Windows li toglie da solo alla creazione, e il
    // percorso che poi si prova a riaprire non è quello che si è chiesto. Vale
    // per gli spazi lasciati dal taglio come per un album «Vol. 1.».
    let mut pulito = pulito.trim_end_matches([' ', '.']).to_owned();
    if pulito.is_empty() {
        return "Senza titolo".to_owned();
    }
    // Un nome riservato da Windows (`CON`, `AUX`…) si disinnesca con un
    // suffisso, non cancellandolo: cancellarlo lascerebbe un nome vuoto, e il
    // ripiego «Senza titolo» qui sopra ha già consumato quel caso.
    let radice = pulito.split('.').next().unwrap_or(&pulito).to_lowercase();
    if NOMI_RISERVATI.contains(&radice.as_str()) {
        pulito.push('_');
    }
    pulito
}

/// Dove va a finire un brano preso, relativamente alla cartella dei download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destinazione {
    /// La cartella dell'interprete.
    pub cartella_artista: String,
    /// La cartella dell'album, dentro quella dell'interprete.
    pub cartella_album: String,
    /// Il nome del file, senza estensione.
    pub nome_base: String,
}

impl Destinazione {
    /// Il percorso relativo, con le barre in avanti.
    ///
    /// In avanti anche su Windows: è la forma che si concatena senza pensarci a
    /// una cartella qualunque, e l'API di sistema le accetta entrambe.
    #[must_use]
    pub fn relativo(&self) -> String {
        format!(
            "{}/{}/{}",
            self.cartella_artista, self.cartella_album, self.nome_base
        )
    }
}

/// Come si chiamerà il file, e in che cartelle starà.
///
/// Si usa l'interprete **dell'album** e non quello del brano: un album con un
/// ospite in tre pezzi resta una cartella sola invece di spargersi in quattro.
/// È lo stesso motivo per cui `album_artist` è la chiave con cui
/// `rebuild_aggregates` raggruppa un album in una sola uscita.
///
/// `numero` è la posizione da cui ricavare il prefisso quando la fonte non dà
/// un numero di traccia, e si conta da 1.
#[must_use]
pub fn destinazione(brano: &BranoEsterno, numero: u32) -> Destinazione {
    let artista = brano
        .album_artist
        .as_deref()
        .or(brano.artist.as_deref())
        .unwrap_or("Sconosciuto");
    // Senza album è un singolo, e «Singoli» è una cartella vera in cui cercarlo,
    // mentre «Album sconosciuto» è un buco travestito da nome.
    let album = brano.album.as_deref().unwrap_or("Singoli");
    let posizione = brano.track_number.unwrap_or(numero);
    Destinazione {
        cartella_artista: segmento_sicuro(artista),
        cartella_album: segmento_sicuro(album),
        nome_base: segmento_sicuro(&format!("{posizione:02} - {}", brano.title)),
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    fn brano(titolo: &str) -> BranoEsterno {
        BranoEsterno {
            title: titolo.to_owned(),
            ..BranoEsterno::default()
        }
    }

    #[test]
    fn i_caratteri_vietati_diventano_spazi_e_non_spariscono() {
        // Sparire attaccherebbe le due parole: «AC/DC» diventerebbe «ACDC», che
        // è un altro nome e non si ritrova cercando.
        assert_eq!(segmento_sicuro("AC/DC"), "AC DC");
        assert_eq!(segmento_sicuro("Q: are we not men?"), "Q are we not men");
    }

    #[test]
    fn un_nome_vuoto_non_resta_vuoto() {
        assert_eq!(segmento_sicuro("   "), "Senza titolo");
        assert_eq!(segmento_sicuro("///"), "Senza titolo");
    }

    #[test]
    fn il_taglio_non_lascia_spazi_in_coda() {
        // Il caso che su Windows crea una cartella che poi non si riapre.
        let lungo = format!("{} fine", "a".repeat(LUNGHEZZA_MASSIMA - 1));
        let tagliato = segmento_sicuro(&lungo);
        assert!(!tagliato.ends_with(' '));
        assert!(tagliato.chars().count() <= LUNGHEZZA_MASSIMA);
    }

    #[test]
    fn senza_album_si_finisce_in_singoli() {
        let d = destinazione(&brano("Karma Police"), 1);
        assert_eq!(d.cartella_album, "Singoli");
        assert_eq!(d.cartella_artista, "Sconosciuto");
        assert_eq!(d.nome_base, "01 - Karma Police");
    }

    #[test]
    fn linterprete_dellalbum_batte_quello_del_brano() {
        let b = BranoEsterno {
            title: "Feel Good Inc.".to_owned(),
            artist: Some("Gorillaz, De La Soul".to_owned()),
            album_artist: Some("Gorillaz".to_owned()),
            album: Some("Demon Days".to_owned()),
            track_number: Some(6),
            ..BranoEsterno::default()
        };
        let d = destinazione(&b, 99);
        assert_eq!(d.cartella_artista, "Gorillaz");
        // Senza il punto finale: Windows lo toglierebbe da solo alla creazione,
        // e qui l'estensione aggiungerà comunque il suo.
        assert_eq!(d.relativo(), "Gorillaz/Demon Days/06 - Feel Good Inc");
    }

    #[test]
    fn i_punti_in_coda_se_ne_vanno() {
        // «Live at Leeds.» creerebbe su Windows una cartella SENZA il punto, e
        // il percorso che Aether prova poi a riaprire non sarebbe quello.
        assert_eq!(segmento_sicuro("Live at Leeds."), "Live at Leeds");
        assert_eq!(segmento_sicuro("Vol. 1..."), "Vol. 1");
    }

    #[test]
    fn i_nomi_riservati_di_windows_si_disinnescano() {
        assert_eq!(segmento_sicuro("AUX"), "AUX_");
        assert_eq!(segmento_sicuro("con"), "con_");
        // Un nome che li contiene soltanto non si tocca.
        assert_eq!(segmento_sicuro("Console"), "Console");
    }

    #[test]
    fn il_numero_di_traccia_batte_la_posizione() {
        let b = BranoEsterno {
            title: "T".to_owned(),
            track_number: Some(3),
            ..BranoEsterno::default()
        };
        assert_eq!(destinazione(&b, 11).nome_base, "03 - T");
    }
}
