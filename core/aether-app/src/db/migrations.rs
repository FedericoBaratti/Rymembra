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
}

/// La catena, in ordine.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline",
        sql: include_str!("schema/001_baseline.sql"),
    },
    Migration {
        version: 2,
        name: "spotify",
        sql: include_str!("schema/002_spotify.sql"),
    },
    Migration {
        version: 3,
        name: "download",
        sql: include_str!("schema/003_download.sql"),
    },
    Migration {
        version: 4,
        name: "arricchimento",
        sql: include_str!("schema/004_arricchimento.sql"),
    },
    Migration {
        version: 5,
        name: "account",
        sql: include_str!("schema/005_account.sql"),
    },
    Migration {
        version: 6,
        name: "scrobble",
        sql: include_str!("schema/006_scrobble.sql"),
    },
    Migration {
        version: 7,
        name: "desiderati",
        sql: include_str!("schema/007_desiderati.sql"),
    },
    Migration {
        version: 8,
        name: "rapporti",
        sql: include_str!("schema/008_rapporti.sql"),
    },
    Migration {
        version: 9,
        name: "sincronia",
        sql: include_str!("schema/009_sincronia.sql"),
    },
    Migration {
        version: 10,
        name: "cataloghi",
        sql: include_str!("schema/010_cataloghi.sql"),
    },
    Migration {
        version: 11,
        name: "testi",
        sql: include_str!("schema/011_testi.sql"),
    },
    Migration {
        version: 12,
        name: "home",
        sql: include_str!("schema/012_home.sql"),
    },
    Migration {
        version: 13,
        name: "testi-senza-tempi",
        sql: include_str!("schema/013_testi_senza_tempi.sql"),
    },
    Migration {
        version: 14,
        name: "home-dischi",
        sql: include_str!("schema/014_home_dischi.sql"),
    },
    Migration {
        version: 15,
        name: "riconcilia",
        sql: include_str!("schema/015_riconcilia.sql"),
    },
    Migration {
        version: 16,
        name: "metadati",
        sql: include_str!("schema/016_metadati.sql"),
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
        #[allow(clippy::indexing_slicing)]
        let version = MIGRATIONS[i].version;
        if version > max {
            max = version;
        }
        i += 1;
    }
    max
};
