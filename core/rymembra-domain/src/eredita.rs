//! I nomi della versione precedente del programma, tenuti solo per ritrovarla.
//!
//! Chi aveva già installato la versione di prima ha dati scritti con i nomi di
//! allora: la cartella dati, il servizio del portachiavi, i file su Google
//! Drive, la firma dei profili, le estensioni, il tag delle modifiche dello
//! Studio. Cambiare nome al programma non li cambia: restano dov'erano finché
//! qualcuno non li sposta. Questo modulo è il catalogo di quei nomi, e serve a
//! una cosa sola: **trovarli e leggerli**, per portare tutto nei nomi nuovi.
//!
//! # La regola
//!
//! Il codice nuovo **non scrive mai** questi nomi. Li cerca, li accetta in
//! lettura, li rinomina in quelli di adesso — e basta. Se in un punto del
//! programma un file, una chiave o un identificativo nasce con uno di questi
//! valori, è un difetto: l'utente si ritroverebbe con metà dei dati sotto un
//! nome e metà sotto l'altro, e senza nessuno che li riunisca.
//!
//! # Perché stanno tutti qui
//!
//! Perché sono nomi che **non si possono più cambiare**: stanno sui dischi e
//! sui server di persone che non li ricontrolleranno. Sparsi per i crate come
//! stringhe letterali, uno si dimenticherebbe al primo rimaneggiamento, e la
//! compatibilità con i dati di prima si romperebbe in silenzio. Raccolti in un
//! posto solo, si contano, si leggono, e un test li confronta con quelli nuovi.
//!
//! Questo è l'unico punto del codice Rust dove compaiono per esteso. Le sole
//! altre eccezioni sono gli alias di `serde` sui campi dei profili, la
//! migrazione SQL che riscrive gli identificativi delle sorgenti, i testi che
//! devono nominare il programma di prima perché l'utente lo chiuda, e i test di
//! compatibilità.

/// L'identificativo dell'applicazione di prima.
///
/// Dà il nome alla cartella dati (`%APPDATA%\<identificativo>`) e al servizio
/// del portachiavi di sistema sotto cui stavano i segreti. Lo legge il
/// trasloco, per spostare la cartella, e il portachiavi, per ripiegare sulle
/// voci vecchie quando quelle nuove mancano.
pub const IDENTIFICATORE: &str = "io.github.federicobaratti.aether";

/// La variabile d'ambiente con cui si sceglieva a mano la cartella dati.
///
/// Chi la teneva impostata (di solito per tenere un'istanza di prova separata
/// da quella vera) continua a vederla rispettata, se la nuova non c'è. La legge
/// `trasloco::scegli_cartella`.
pub const VARIABILE_DATI: &str = "AETHER_DATI";

/// Il nome del file del database di prima, dentro la cartella dati.
///
/// Con lui vanno i suoi `-wal` e `-shm` e le copie di sicurezza
/// `<nome>.schema-NNN.bak`. Lo rinomina il trasloco, **per ultimo**: finché
/// esiste, nessun codice nuovo ha aperto quel database.
pub const NOME_DATABASE: &str = "aether.db";

/// La parte fissa del nome del diario di bordo di prima (`<stem>.log`,
/// `<stem>.1.log`, ...), dentro la cartella `diario`.
///
/// Lo rinomina il trasloco, perché la rotazione dei file nuovi parta da una
/// serie sola.
pub const STEM_DIARIO: &str = "aether";

/// L'estensione dei pacchetti di skin di prima, senza il punto.
///
/// La leggono l'importazione (finestra di scelta e trascinamento), la
/// classificazione dei profili e il trasloco, che rinomina le skin già
/// installate. In scrittura si usa solo l'estensione nuova.
pub const ESTENSIONE_SKIN: &str = "aeskin";

/// L'estensione dei profili esportati di prima, senza il punto.
///
/// La leggono l'importazione del profilo e il trasloco, che rinomina le copie
/// di sicurezza `prima-*` della cartella `profilo`. In scrittura si usa solo
/// l'estensione nuova.
pub const ESTENSIONE_PROFILO: &str = "aeprofile";

/// La firma che i profili di prima portavano nel manifesto.
///
/// Un profilo è valido se la sua firma è quella nuova **o** questa; una firma
/// diversa da entrambe resta rifiutata. La controlla il lettore dell'archivio
/// del profilo.
pub const FIRMA_PROFILO: &str = "aether.profilo";

/// Il prefisso con cui i dispositivi di prima nominavano i loro documenti di
/// sincronizzazione (`<prefisso><id dispositivo><suffisso>`).
///
/// Lo legge il motore di sincronia per riconoscere i documenti, anche quelli
/// del dispositivo stesso rimasti col nome vecchio, e la classificazione dei
/// profili. Chi scrive usa solo il prefisso nuovo.
pub const PREFISSO_DISPOSITIVO: &str = "aether-";

/// Il nome del file della libreria su Google Drive (cartella dati dell'app),
/// com'era prima.
///
/// Lo cerca il servizio cloud: se c'è solo questo, lo rinomina o lo aggiorna
/// col nome nuovo; se ci sono entrambi, vince il nuovo.
pub const NOME_LIBRERIA_DRIVE: &str = "aether-libreria.v1.json.gz";

/// Il prefisso dei pacchetti di skin su Google Drive, com'era prima
/// (`<prefisso><id>.<estensione skin>`).
///
/// Lo legge il servizio cloud per riconoscere le skin già caricate e portarle
/// al nome nuovo.
pub const PREFISSO_SKIN_DRIVE: &str = "aether-skin-";

/// Il prefisso delle bozze delle skin su Google Drive, com'era prima
/// (`<prefisso><id>.json.gz`).
///
/// Lo legge il servizio cloud per riconoscere le bozze già caricate e portarle
/// al nome nuovo.
pub const PREFISSO_BOZZA_DRIVE: &str = "aether-bozza-";

/// Il prefisso degli identificativi di sorgente di prima (`<prefisso>spotify:...`,
/// `<prefisso>archivio:...`), che marca le righe nate da un'importazione.
///
/// Lo riscrive la migrazione del database e lo normalizza l'importazione di un
/// profilo vecchio. Le righe nuove nascono col prefisso nuovo.
pub const PREFISSO_SORGENTE: &str = "aether:";

/// Il linguaggio del blocco di codice con cui l'IA dello Studio proponeva le
/// modifiche, com'era prima (il tag che segue i tre apici).
///
/// Lo accetta il lettore dei blocchi: le chat salvate e la storia rimandata al
/// modello lo contengono ancora. Al modello si insegna, e dal modello si
/// riceve, solo il tag nuovo.
pub const LINGUAGGIO_PATCH: &str = "aether-patch";

#[cfg(test)]
mod prove {
    use super::*;

    /// Ogni nome ha il suo omologo di adesso, e deve esserne diverso: se un
    /// giorno uno dei due diventasse uguale all'altro, la migrazione
    /// «rinomina solo se il vecchio c'è e il nuovo no» non avrebbe più niente
    /// da distinguere, e un conflitto passerebbe per un file già a posto.
    #[test]
    fn i_nomi_di_prima_differiscono_da_quelli_di_adesso() {
        let coppie = [
            (IDENTIFICATORE, "io.github.federicobaratti.rymembra"),
            (VARIABILE_DATI, "RYMEMBRA_DATI"),
            (NOME_DATABASE, "rymembra.db"),
            (STEM_DIARIO, "rymembra"),
            (ESTENSIONE_SKIN, "ryskin"),
            (ESTENSIONE_PROFILO, "ryprofile"),
            (FIRMA_PROFILO, "rymembra.profilo"),
            (PREFISSO_DISPOSITIVO, "rymembra-"),
            (NOME_LIBRERIA_DRIVE, "rymembra-libreria.v1.json.gz"),
            (PREFISSO_SKIN_DRIVE, "rymembra-skin-"),
            (PREFISSO_BOZZA_DRIVE, "rymembra-bozza-"),
            (PREFISSO_SORGENTE, "rymembra:"),
            (LINGUAGGIO_PATCH, "rymembra-patch"),
        ];
        for (prima, adesso) in coppie {
            assert_ne!(prima, adesso, "{prima} deve differire dal nome nuovo");
            assert!(!prima.is_empty());
        }
    }

    /// I prefissi di Drive e di sincronia sono quelli di un nome intero: se
    /// uno perdesse il trattino finale o i due punti, `strip_prefix` smetterebbe
    /// di separare il prefisso dall'identificativo.
    #[test]
    fn i_prefissi_chiudono_con_il_loro_separatore() {
        for prefisso in [
            PREFISSO_DISPOSITIVO,
            PREFISSO_SKIN_DRIVE,
            PREFISSO_BOZZA_DRIVE,
        ] {
            assert!(prefisso.ends_with('-'), "{prefisso}");
        }
        assert!(PREFISSO_SORGENTE.ends_with(':'));
        assert!(NOME_LIBRERIA_DRIVE.starts_with(PREFISSO_DISPOSITIVO));
        assert!(NOME_LIBRERIA_DRIVE.ends_with(".json.gz"));
    }

    /// Le estensioni si usano senza punto, come fa il resto del programma.
    #[test]
    fn le_estensioni_non_hanno_il_punto() {
        assert!(!ESTENSIONE_SKIN.contains('.'));
        assert!(!ESTENSIONE_PROFILO.contains('.'));
    }
}
