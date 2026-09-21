//! La catena delle migrazioni: aggiungere una riga in fondo, mai in mezzo.
//!
//! Ogni voce ha un numero che diventa `PRAGMA user_version` una volta applicata.
//! I numeri sono consecutivi da 1 e un test lo verifica: un buco significa che
//! due rami hanno aggiunto una migrazione ciascuno con lo stesso numero, e i due
//! database che ne risultano si direbbero alla stessa versione avendo colonne
//! diverse — un guasto che si manifesta molto dopo, come query che falliscono
//! su un dispositivo solo.
//!
//! Il SQL sta in file separati, non in stringhe dentro il Rust: così lo si legge
//! con l'evidenziazione della sintassi, e un `git diff` di una migrazione mostra
//! lo schema che cambia invece di una stringa che cambia.

/// Un passo della catena.
pub struct Migration {
    /// Il numero che finisce in `PRAGMA user_version`. Consecutivo da 1.
    pub version: u32,
    /// Come si chiama, per i log e per il messaggio d'errore se fallisce.
    pub name: &'static str,
    /// Il SQL da eseguire. Gira già dentro una transazione.
    pub sql: &'static str,
    /// Questa migrazione **ricostruisce** una tabella: la crea da capo, ci
    /// travasa dentro i dati, butta l'originale e rinomina.
    ///
    /// Serve a saperlo perché SQLite non sa togliere un `NOT NULL` in altro
    /// modo, e perché quel `DROP TABLE` è pericoloso in un modo che non si
    /// vede: con le chiavi esterne accese esegue una cancellazione implicita
    /// che fa scattare tutte le `ON DELETE CASCADE` che puntano alla tabella.
    /// Su `tracks` vorrebbe dire cronologia d'ascolto, playlist e correzioni
    /// cancellate in silenzio, dentro una migrazione che riporta successo.
    ///
    /// Quando è `true`, [`super::migrate`] spegne `foreign_keys` prima di
    /// aprire la transazione e verifica con `PRAGMA foreign_key_check`, prima
    /// di chiuderla, che non sia rimasto niente a puntare nel vuoto. Per tutte
    /// le altre — cioè per quasi tutte — non cambia niente.
    pub ricostruisce: bool,
}

/// La catena, in ordine.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline",
        sql: include_str!("schema/001_baseline.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 2,
        name: "spotify",
        sql: include_str!("schema/002_spotify.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 3,
        name: "download",
        sql: include_str!("schema/003_download.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 4,
        name: "arricchimento",
        sql: include_str!("schema/004_arricchimento.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 5,
        name: "account",
        sql: include_str!("schema/005_account.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 6,
        name: "scrobble",
        sql: include_str!("schema/006_scrobble.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 7,
        name: "desiderati",
        sql: include_str!("schema/007_desiderati.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 8,
        name: "rapporti",
        sql: include_str!("schema/008_rapporti.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 9,
        name: "sincronia",
        sql: include_str!("schema/009_sincronia.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 10,
        name: "cataloghi",
        sql: include_str!("schema/010_cataloghi.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 11,
        name: "testi",
        sql: include_str!("schema/011_testi.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 12,
        name: "home",
        sql: include_str!("schema/012_home.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 13,
        name: "testi-senza-tempi",
        sql: include_str!("schema/013_testi_senza_tempi.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 14,
        name: "home-dischi",
        sql: include_str!("schema/014_home_dischi.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 15,
        name: "riconcilia",
        sql: include_str!("schema/015_riconcilia.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 16,
        name: "metadati",
        sql: include_str!("schema/016_metadati.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 17,
        name: "affinita",
        sql: include_str!("schema/017_affinita.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 18,
        name: "settimana",
        sql: include_str!("schema/018_settimana.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 19,
        name: "identita-e-metadati",
        sql: include_str!("schema/019_identita_e_metadati.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 20,
        name: "cronologia-unica",
        sql: include_str!("schema/020_cronologia_unica.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 21,
        name: "impronte-illeggibili",
        sql: include_str!("schema/021_impronte_illeggibili.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 22,
        name: "testi-seguono-il-brano",
        sql: include_str!("schema/022_testi_seguono_il_brano.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 23,
        name: "correzioni-orfane",
        sql: include_str!("schema/023_correzioni_orfane.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 24,
        name: "indici-degli-ordinamenti",
        sql: include_str!("schema/024_indici_degli_ordinamenti.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 25,
        name: "testi-scelti-e-scartati",
        sql: include_str!("schema/025_testi_scelti_e_scartati.sql"),
        ricostruisce: false,
    },
    Migration {
        version: 26,
        name: "brani-di-catalogo",
        sql: include_str!("schema/026_brani_di_catalogo.sql"),
        // L'unica finora, e il commento in testa al file dice cosa costa.
        ricostruisce: true,
    },
    Migration {
        version: 27,
        name: "bitrate-da-rileggere",
        sql: include_str!("schema/027_bitrate_da_rileggere.sql"),
        ricostruisce: false,
    },
];

/// La versione a cui questa build porta il database.
pub const LATEST_VERSION: u32 = {
    // Calcolata dalla catena invece che scritta a mano: due numeri da tenere
    // allineati sono due numeri che prima o poi divergono, e questo in
    // particolare deciderebbe di non aprire un database perfettamente valido.
    let mut max = 0;
    let mut i = 0;
    while i < MIGRATIONS.len() {
        // `indexing_slicing` è vietato nel codice normale, ma in un contesto
        // `const` non esistono ancora `get()` fallibili: l'indice è comunque
        // limitato dalla condizione del ciclo.
        #[expect(
            clippy::indexing_slicing,
            reason = "in const non esiste un get() fallibile, e i < MIGRATIONS.len()"
        )]
        let version = MIGRATIONS[i].version;
        if version > max {
            max = version;
        }
        i += 1;
    }
    max
};
