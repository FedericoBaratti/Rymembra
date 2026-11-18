//! Il ponte fra il database e la fusione.
//!
//! `rymembra-sync` decide cosa sopravvive quando due dispositivi si incontrano, e
//! per farlo non ha bisogno di sapere che esiste SQLite: riceve documenti e ne
//! restituisce uno stato. Questo modulo è l'unico posto in tutto l'albero che
//! sa tradurre fra le due lingue — righe da una parte, mappe dall'altra.
//!
//! Tre funzioni, in quest'ordine e per un motivo:
//!
//! 1. [`allinea`] porta le tabelle della sincronia in pari con la libreria. È
//!    l'unica scrittura che avviene *prima* di guardare fuori, e serve perché il
//!    resto dell'applicazione continua a scrivere dove ha sempre scritto: chi
//!    aggiunge un brano a una playlist tocca `playlist_tracks`, non
//!    `sync_sequenza`, e ha ragione a farlo. Fa anche il percorso inverso in un
//!    solo caso: un brano che la sequenza nomina da prima e che qui è arrivato
//!    dopo rientra in `playlist_tracks` al suo posto (vedi
//!    [`allinea_sequenze`]: un brano che qui non c'è è invisibile, non tolto).
//! 2. [`contenuto`] costruisce il documento da pubblicare. Sola lettura.
//! 3. [`applica`] scrive quel che la fusione ha deciso.
//!
//! La terza è in due pezzi, e non è una quarta funzione: [`applica_in`] fa le
//! scritture dentro una transazione **già aperta**, e [`applica`] è l'involucro
//! che quella transazione la apre e la chiude. Serve al profilo, che deve poter
//! eseguire le stesse scritture dentro un punto di salvataggio e abbandonarlo
//! per mostrare un piano — vedi il `//!` di [`crate::profilo`].
//!
//! Fra la seconda e la terza c'è la rete, e il lucchetto della libreria **non**
//! ci sta attorno. È la stessa disciplina che `nuvola.rs` documenta da tempo, e
//! la ragione per cui queste sono tre funzioni invece di una.
//!
//! # Perché l'allineamento è pigro e non un obbligo per chi scrive
//!
//! L'alternativa sarebbe far scrivere `sync_ascolti` e `sync_sequenza` a ogni
//! percorso che tocca la libreria: l'importazione dal vecchio database, quella
//! da un account, la scansione, i comandi delle playlist. Sono una dozzina di
//! posti, e ognuno di loro sarebbe un posto in cui *dimenticarsene* — con il
//! sintomo che compare settimane dopo, su un dispositivo diverso, come un
//! conteggio che non torna.
//!
//! Qui invece la verità resta una sola: `tracks.play_count` e `playlist_tracks`
//! restano ciò che sono sempre stati, e la sincronia si allinea a loro. Chi
//! importa non cambia di una riga. Il prezzo è una passata in più sui dati
//! all'inizio di ogni sincronizzazione, ed è un prezzo in millisecondi.
//!
//! # Cosa questo modulo NON fa, e non è una dimenticanza
//!
//! Non apre file, non parla con la rete e non decide niente. Il deposito è un
//! [`rymembra_sync::Magazzino`], la decisione è in [`rymembra_sync::fondi`], e chi
//! mette insieme le tre cose è la finestra. Qui c'è solo la traduzione.
//!
//! # «Ascolta insieme» non viaggia, e nemmeno passa di qui
//!
//! I PC associati (`insieme_pari`) e le chiavi `insieme.*` non entrano nel
//! documento, e non per una riga che li escluda: [`contenuto`] sceglie campo
//! per campo cosa leggere, e delle impostazioni legge solo le cartelle e la
//! skin. È voluto. Un'associazione è una fiducia data da questo PC a una
//! chiave dopo un codice confermato su due schermi; arrivata col documento su
//! un telefono o su un altro computer, direbbe che si fida chi non ha mai
//! visto quel codice. E non va confusa con [`dispositivi`]: quella è la fiducia
//! nei documenti di chi scrive nello stesso Drive, un'altra domanda con
//! un'altra tabella. Una prova in fondo tiene vere tutte e due le cose.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use rusqlite::{Connection, Transaction};
use rymembra_domain::errors::AppError;
use rymembra_sync::contatore::IMPORTAZIONE;
use rymembra_sync::{
    Contenuto, Elemento, Fuso, Identita, Interruttore, Lapidi, Momento, PlaylistSincronizzata,
    Scelta, Sequenza, Voto,
};

use crate::library::db_error;
use crate::settings::{
    self, CHIAVE_CARTELLE, CHIAVE_CARTELLE_SINCRONIA, CHIAVE_SKIN, CHIAVE_SKIN_QUANDO,
};

/// Il dispositivo fittizio con cui si datano le sequenze già esistenti.
///
/// Una playlist nata prima che ci fosse una sincronia non ha identità per i suoi
/// elementi, e bisogna dargliele. La tentazione è usare il proprio nome di
/// dispositivo; sarebbe sbagliato, e in un modo che si vede solo al primo
/// incontro fra due macchine.
///
/// Due dispositivi che hanno la **stessa** playlist — perché l'hanno importata
/// dallo stesso vecchio database — la battezzerebbero ciascuno a modo proprio,
/// `portatile:0…9` e `telefono:0…9`. Sono venti elementi distinti per dieci
/// brani, e la fusione li terrebbe tutti: la playlist raddoppierebbe alla prima
/// passata, con ogni canzone due volte.
///
/// Con un nome uguale per tutti le identità coincidono, l'unione è un'operazione
/// a vuoto, e la playlist resta di dieci.
///
/// Il rovescio, ed è un limite dichiarato e non il comportamento voluto: due
/// playlist con lo stesso nome ma elenchi **diversi**, battezzate ciascuna per
/// conto suo, non si uniscono. `origine:k` di qua e `origine:k` di là sono per
/// la fusione lo stesso elemento, e dove i brani in quella posizione
/// differiscono [`Sequenza`] ne tiene uno solo (la coppia minore): `[x, y]` e
/// `[z, w]` diventano due brani, non quattro. Vale fra due dispositivi che si
/// sincronizzano, e la prova qui in fondo fissa il caso; il profilo non ci
/// passa, perché dentro una playlist che c'è già accoglie soltanto in coda i
/// brani che le mancano, con l'identità che hanno nel profilo (vedi
/// `profilo::dono_nelle_playlist`).
///
/// Vale **solo per il primo battesimo** ([`Sequenza::dalla_lista`]). Ciò che si
/// aggiunge o si sposta dopo porta il nome del dispositivo: vedi
/// [`allinea_sequenze`].
const ORIGINE: &str = "origine";

/// Dove si ricorda quali brani le sequenze nominavano senza che qui ci fossero.
///
/// È la memoria che permette a [`allinea_sequenze`] di distinguere due cose che
/// nel database sono identiche. Una voce viva di sequenza il cui brano è in
/// `tracks` ma non è in `playlist_tracks` può essere
///
/// - un brano che l'utente ha **tolto** dalla playlist, e allora si seppellisce
///   e la lapide viaggia;
/// - un brano che l'altro dispositivo aveva messo nella playlist e che qui è
///   **arrivato dopo** (una scansione, la sorveglianza, «Scarica qui», una
///   correzione che ne ha cambiato la chiave): nessuno l'ha tolto, e va messo in
///   `playlist_tracks` al suo posto.
///
/// Fra i due non c'è nessuna traccia nelle tabelle, e non ce ne può essere una
/// senza una migrazione. C'è questa: le chiavi che, all'ultima volta che una
/// sequenza è stata allineata o ha ricevuto voci, qualche voce — viva **o
/// tolta**, vedi [`brani_assenti`] — nominava e `tracks` non aveva. Una voce di
/// quelle chiavi che oggi ha il brano è «da agganciare»; una voce di una chiave
/// che non ci stava, no.
///
/// Serve anche al caso opposto, per lo stesso motivo: se una chiave della
/// memoria è tornata e `playlist_tracks` la nomina senza che la sequenza abbia
/// una voce viva per lei, a rimetterla è stato il trigger della migrazione 28,
/// e la voce era stata tolta altrove mentre il file non c'era. Non è
/// un'aggiunta: la riga si toglie (vedi [`senza_i_ritorni_di_voci_tolte`]).
///
/// Si riscrive in coda a [`allinea_in`] e a [`applica_in`], cioè in ogni punto
/// in cui `sync_sequenza` cambia o `tracks` viene confrontata con lei, e nella
/// stessa transazione: un piano di profilo che si abbandona, o una passata che
/// non si applica, non lasciano una memoria che non corrisponde ai dati.
///
/// Sta in `settings` e non in una tabella: stato di questo dispositivo, che non
/// viaggia (il profilo porta un elenco di chiavi, e questa non c'è) e si
/// ricostruisce da sé.
///
/// # Assente e illeggibile sono due cose
///
/// Se **manca** (nessun ricordo: è lo stato normale di chi ha tutta la musica, e
/// della prima passata dopo l'aggiornamento) vale «niente da agganciare»: un
/// brano arrivato prima del primo `allinea` che la riscrive si legge come
/// tolto, che è il comportamento di prima. Succede una volta sola, e non a
/// regime.
///
/// Se **c'è e non si legge** (un valore guasto, un ripristino del database da
/// un file di prima) non si sa niente, e non sapere non è sapere che niente è
/// arrivato: [`Ricordo::Illeggibile`]. In quella passata non si seppellisce
/// nessuna voce — una lapide che parte per un brano che un altro dispositivo
/// ha è la perdita che tutto questo esiste per evitare — e i brani che qui ci
/// sono, hanno una voce viva e non stanno in `playlist_tracks` si riagganciano
/// tutti, anche quelli che l'utente aveva appena tolto: l'errore sicuro è una
/// voce che torna, non un brano che sparisce dagli altri dispositivi. La
/// memoria si rifà in coda alla passata, e dalla successiva si torna a sapere.
const CHIAVE_BRANI_ASSENTI: &str = "sincronia.brani_assenti";

// ── il conteggio d'ascolto ──────────────────────────────────────────────────

/// Segna un ascolto a nome di questo dispositivo.
///
/// Due scritture e non una: sale il contatore di chi ha ascoltato, e
/// `tracks.play_count` si rifà come somma. La seconda esiste perché tutte le
/// interrogazioni della libreria continuano a leggere quella colonna e non
/// devono accorgersi di niente — sono più di cento, e riscriverle sarebbe stato
/// il modo di introdurre cento occasioni di sbagliare per un cambiamento che qui
/// costa una riga.
///
/// La somma è su `track_key` e non su `id`: due file dello stesso brano sono lo
/// stesso brano, ed è già così che `backup.rs` li tratta quando salva.
///
/// # Prima della somma, lo storico che nessuno rivendica
///
/// Rifare la colonna come somma è giusto solo se la somma conosce **tutto**
/// quel che la colonna dice. Il ripristino da backup, l'importazione di un
/// account e quella dal vecchio database scrivono `play_count` e non toccano
/// `sync_ascolti`; l'eccedenza diventa di `importazione` solo in [`allinea`],
/// che gira con la sincronia accesa e nel profilo. Con la sincronia spenta, che
/// è il default, il primo ascolto dopo un ripristino riscriveva 40 come 1. Per
/// questo qui si fa, per la sola chiave del brano, la stessa attribuzione di
/// [`allinea_ascolti`]: stessa sottrazione, stesso `max`, quindi a vuoto quando
/// la colonna è già in pari e mai al ribasso. Costa una lettura indicizzata.
///
/// # Errori
///
/// `db.queryFailed` se una delle scritture fallisce.
pub fn conta_ascolto(
    tx: &Transaction<'_>,
    track_id: i64,
    dispositivo: &str,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO sync_ascolti (track_key, dispositivo, quanti)
         SELECT chiave, ?2, eccedenza FROM (
           SELECT t.track_key AS chiave,
                  MAX(t.play_count) - COALESCE((
                    SELECT SUM(a.quanti) FROM sync_ascolti a
                     WHERE a.track_key = t.track_key AND a.dispositivo <> ?2
                  ), 0) AS eccedenza
             FROM tracks t
            WHERE t.track_key = (SELECT track_key FROM tracks WHERE id = ?1)
              AND NOT (t.path >= 'lontano://' AND t.path < 'lontano:/0')
            GROUP BY t.track_key
         )
         WHERE eccedenza > 0
         ON CONFLICT(track_key, dispositivo) DO UPDATE SET
           quanti = max(sync_ascolti.quanti, excluded.quanti)",
        rusqlite::params![track_id, IMPORTAZIONE],
    )
    .map_err(|err| db_error("storico ereditato del brano", &err))?;

    tx.execute(
        "INSERT INTO sync_ascolti (track_key, dispositivo, quanti)
         SELECT track_key, ?2, 1 FROM tracks WHERE id = ?1
         ON CONFLICT(track_key, dispositivo) DO UPDATE SET quanti = quanti + 1",
        rusqlite::params![track_id, dispositivo],
    )
    .map_err(|err| db_error("conteggio per dispositivo", &err))?;

    tx.execute(
        "UPDATE tracks
            SET play_count = COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                        WHERE a.track_key = tracks.track_key), 0)
          WHERE track_key = (SELECT track_key FROM tracks WHERE id = ?1)
            AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
        rusqlite::params![track_id],
    )
    .map_err(|err| db_error("somma degli ascolti", &err))?;
    Ok(())
}

/// Ricorda dove si era arrivati in un brano lasciato a metà.
///
/// Solo il più recente vince, e non il più avanti: chi ha riascoltato ieri sa
/// meglio dove si è fermato di chi il mese scorso era andato più in là.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn segna_posizione(
    connection: &Connection,
    track_key: &str,
    ms: i64,
    adesso_ms: i64,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO sync_posizioni (track_key, ms, at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_key) DO UPDATE SET ms = excluded.ms, at_ms = excluded.at_ms
             WHERE excluded.at_ms >= sync_posizioni.at_ms",
            rusqlite::params![track_key, ms.max(0), adesso_ms],
        )
        .map_err(|err| db_error("posizione di riascolto", &err))?;
    Ok(())
}

// ── allineamento ────────────────────────────────────────────────────────────

/// Quante cose l'allineamento ha dovuto mettere in pari.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Allineato {
    /// Brani il cui storico ereditato è cambiato.
    pub ascolti: usize,
    /// Playlist la cui sequenza è stata rifatta.
    pub sequenze: usize,
    /// Playlist in cui sono rientrati brani che qui sono arrivati dopo la loro
    /// voce, o da cui è uscita la riga di un brano tornato che la sequenza non
    /// giustificava: la sequenza è rimasta com'era, è `playlist_tracks` che si
    /// è messa d'accordo con lei. Vedi [`allinea_sequenze`].
    pub agganciate: usize,
    /// Cartelle accese o spente da qui.
    pub cartelle: usize,
}

impl Allineato {
    /// Non c'era niente da mettere in pari.
    #[must_use]
    pub const fn e_vuoto(&self) -> bool {
        self.ascolti == 0 && self.sequenze == 0 && self.agganciate == 0 && self.cartelle == 0
    }
}

/// Porta le tabelle della sincronia in pari con la libreria.
///
/// Da chiamare all'inizio di una passata, prima di [`contenuto`]. Su una
/// libreria ferma non scrive niente e costa nove interrogazioni: due sugli
/// ascolti, cinque sulle sequenze (i membri delle playlist in due, le
/// sequenze salvate, i brani assenti e la loro memoria, una per una) e due
/// sulle cartelle.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub fn allinea(
    connection: &mut Connection,
    dispositivo: &str,
    adesso_ms: i64,
) -> Result<Allineato, AppError> {
    let tx = crate::library::transazione_di_scrittura(connection)
        .map_err(|err| db_error("apertura dell'allineamento", &err))?;

    let fatto = allinea_in(&tx, dispositivo, adesso_ms)?;

    tx.commit()
        .map_err(|err| db_error("chiusura dell'allineamento", &err))?;
    Ok(fatto)
}

/// Le scritture di [`allinea`], dentro una transazione già aperta: la usa anche
/// [`applica_se_ferma`].
///
/// `dispositivo` è il nome con cui si battezzano gli elementi che una modifica
/// fatta qui aggiunge a una sequenza già esistente: vedi [`allinea_sequenze`].
fn allinea_in(
    tx: &Transaction<'_>,
    dispositivo: &str,
    adesso_ms: i64,
) -> Result<Allineato, AppError> {
    let ascolti = allinea_ascolti(tx)?;
    // La memoria dei brani assenti la riscrive `allinea_sequenze` in coda, con
    // quel che ha già calcolato: vedi [`CHIAVE_BRANI_ASSENTI`].
    let (sequenze, agganciate) = allinea_sequenze(tx, dispositivo, adesso_ms)?;
    Ok(Allineato {
        ascolti,
        sequenze,
        agganciate,
        cartelle: allinea_cartelle(tx, adesso_ms)?,
    })
}

/// Riporta sotto `importazione` tutto lo storico che nessun dispositivo rivendica.
///
/// `tracks.play_count` può crescere per vie che la sincronia non vede:
/// un'importazione dal vecchio database, o l'archivio che un servizio spedisce.
/// Quella crescita è storia vera e non va persa, ma non appartiene a questo
/// dispositivo — appartiene a `importazione`, che è lo stesso pseudo-dispositivo
/// ovunque e si fonde col massimo.
///
/// Il calcolo è una sottrazione: quel che la colonna dice, meno quel che i
/// dispositivi rivendicano, è quel che resta da attribuire. Non scende mai —
/// `max` nell'upsert — perché una diminuzione qui significherebbe buttare via
/// ascolti sulla base di un numero che potrebbe essere solo arrivato tardi.
fn allinea_ascolti(tx: &Transaction<'_>) -> Result<usize, AppError> {
    let toccate = tx
        .execute(
            "INSERT INTO sync_ascolti (track_key, dispositivo, quanti)
             SELECT chiave, ?1, eccedenza FROM (
               SELECT t.track_key AS chiave,
                      MAX(t.play_count) - COALESCE((
                        SELECT SUM(a.quanti) FROM sync_ascolti a
                         WHERE a.track_key = t.track_key AND a.dispositivo <> ?1
                      ), 0) AS eccedenza
                 FROM tracks t
                WHERE NOT (t.path >= 'lontano://' AND t.path < 'lontano:/0')
                GROUP BY t.track_key
             )
             WHERE eccedenza > 0
             ON CONFLICT(track_key, dispositivo) DO UPDATE SET
               quanti = max(sync_ascolti.quanti, excluded.quanti)",
            [IMPORTAZIONE],
        )
        .map_err(|err| db_error("storico ereditato", &err))?;

    // Il `WHERE` non è un'ottimizzazione da manuale: `tracks` ha un trigger che
    // rifà l'indice di ricerca a **ogni** UPDATE, anche quando il valore scritto
    // è identico a quello che c'era. Senza la guardia, un allineamento a vuoto su
    // diecimila brani ricostruirebbe diecimila righe di FTS ogni volta.
    tx.execute(
        "UPDATE tracks
            SET play_count = COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                        WHERE a.track_key = tracks.track_key), 0)
          WHERE play_count <> COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                         WHERE a.track_key = tracks.track_key), 0)
            AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
        [],
    )
    .map_err(|err| db_error("somma degli ascolti", &err))?;
    Ok(toccate)
}

/// Le sequenze delle playlist, in pari con `playlist_tracks`.
///
/// Chi aggiunge o sposta un brano scrive `playlist_tracks` e non sa che esista
/// una sequenza; qui si guarda la differenza fra l'ordine che la sequenza
/// produce e quello che la tabella dice, e la si riversa con
/// [`Sequenza::riordina_lasciando`] — che tocca solo ciò che si è mosso davvero
/// invece di rifare tutto, e quindi non lascia una lapide per brano a ogni
/// trascinamento.
///
/// # Due nomi, per due momenti diversi
///
/// Il primo battesimo di una playlist usa [`ORIGINE`]; tutto ciò che si
/// aggiunge **dopo** porta il nome di questo dispositivo. Non è un dettaglio: il
/// numero di un elemento nuovo è il massimo più uno, uguale su due dispositivi
/// allineati, e con lo stesso nome per tutti due aggiunte fatte in
/// contemporanea — `d` di qua, `e` di là — nascevano entrambe `origine:3`. La
/// fusione tratta un'identità uguale come lo stesso elemento e ne tiene uno: `e`
/// spariva da tutti e due, in silenzio, e uno spostamento in testa contro un
/// inserimento in testa faceva sparire il brano spostato. Col nome del
/// dispositivo sono `portatile:3` e `fisso:3`, due elementi, e la fusione li
/// tiene entrambi; il prezzo è che la stessa aggiunta fatta in due posti
/// scollegati resta due volte, che è ciò che [`Sequenza`] promette per gli
/// inserimenti concorrenti ed è meglio di una perdita.
///
/// # Un brano che qui non c'è è invisibile, non tolto
///
/// `playlist_tracks` nomina solo brani che questa libreria ha: ogni riga punta a
/// `tracks`, e [`membri_per_playlist`] lo ripete con il `JOIN`. La sequenza,
/// invece, è lo stato che si **scambia** con gli altri dispositivi, e nomina
/// anche i brani che solo loro hanno: il telefono con una parte della musica del
/// computer, un file cancellato o spostato fuori dalle cartelle su una macchina
/// sola. Confrontare i due elenchi e riversare la differenza come faceva
/// `riordina` voleva dire leggere ogni voce di un brano assente come «l'utente
/// l'ha tolto»: la voce veniva sepolta, la lapide partiva col documento
/// successivo, e per la regola di fusione («la più vecchia vince») i
/// dispositivi che il brano ce l'hanno lo toglievano dalla **loro** playlist.
///
/// Quindi qui una voce il cui brano non c'è in `tracks` si **lascia stare**:
/// viva, con la sua identità, al suo posto nell'ordine che si pubblica. E il
/// giorno che il brano arriva, per qualunque via (la scansione, la
/// sorveglianza, «Scarica qui», una correzione che ne cambia la chiave, un
/// profilo), la voce ha già il suo posto e basta agganciarlo.
///
/// # «Qui» vuol dire due cose, e una riga specchio le distingue
///
/// Un brano che c'è **solo come riga specchio** — il brano di un altro
/// dispositivo, `lontano://…` — è, per **agganciare**, presente: una voce viva
/// entra in `playlist_tracks` anche su una riga del PC, o sul telefono le
/// playlist perderebbero i brani del PC. Ma per **seppellire** è assente: una
/// riga specchio nasce, cambia chiave e sparisce per mano del proprietario e
/// della rete, e che la sua chiave resti per un'altra riga (la copia di qui con
/// un tag corretto, che sul PC ha ancora la vecchia chiave) non dice niente di
/// cosa l'utente ha fatto della playlist. La differenza fra le tabelle, per
/// quelle voci, non è una lapide: lo è solo il gesto dell'utente, e il gesto la
/// scrive da sé ([`seppellisci_le_voci_tolte`]). Vedi [`brani_assenti`].
///
/// Agganciarlo è la parte difficile: al primo `allinea` dopo l'arrivo il brano
/// c'è, la voce è viva e `playlist_tracks` non lo nomina, cioè è identico a un
/// brano che l'utente ha appena tolto. Si distingue con
/// [`CHIAVE_BRANI_ASSENTI`]: se la chiave era fra quelle che non c'erano
/// all'ultimo allineamento è arrivata, se no è stata tolta. Le voci arrivate
/// rientrano in `playlist_tracks` al loro posto (quello che la sequenza dà loro,
/// con l'ordine che l'utente ha fatto qui già riversato) nella stessa
/// transazione, e un brano già messo a mano non si raddoppia: le voci si
/// contano per brano, come fa [`Sequenza::riordina_lasciando`].
///
/// Il limite, dichiarato: un brano tolto da una playlist fra il suo arrivo e il
/// primo `allinea` dopo ricompare. Non è un percorso comune (finché
/// l'`allinea` non lo aggancia, il brano non sta nella playlist e l'utente non
/// lo vede da togliere; fa eccezione il trigger della migrazione 28, che rimette
/// il posto di un file tornato), ed è l'errore sicuro: una voce che torna, non
/// un brano che sparisce dagli altri dispositivi.
///
/// Il rovescio dell'aggancio: un file che torna e ha un posto in `playlist_tracks`
/// per via del trigger, mentre la sequenza ne ha solo lapidi (la voce è stata
/// tolta su un altro dispositivo mentre il file non c'era), non è un'aggiunta
/// ma il ritorno di una voce tolta, e la riga si toglie invece di diventare una
/// voce nuova. Il rischio inverso e la scelta stanno in
/// [`senza_i_ritorni_di_voci_tolte`].
///
/// Se la memoria c'è e non si legge ([`Ricordo::Illeggibile`]) non si sa quali
/// brani sono arrivati, e per quella passata non si seppellisce niente: tutte
/// le voci vive di brani che qui ci sono e non stanno in `playlist_tracks`
/// rientrano, anche quelle che l'utente aveva tolto un attimo prima. Stessa
/// scelta, stesso lato dell'errore.
///
/// Restituisce quante sequenze si sono rifatte e in quante playlist sono
/// rientrati dei brani.
fn allinea_sequenze(
    tx: &Transaction<'_>,
    dispositivo: &str,
    adesso_ms: i64,
) -> Result<(usize, usize), AppError> {
    let dispositivo = nome_delle_aggiunte(dispositivo);
    let membri = membri_per_playlist(tx)?;
    let mut sequenze = sequenze_salvate(tx)?;
    let assenti = brani_assenti(tx)?;
    let ricordo = ricordo_dei_brani_assenti(tx)?;
    // Un ricordo illeggibile non dice niente: vedi [`Ricordo::Illeggibile`].
    let illeggibile = ricordo == Ricordo::Illeggibile;
    let nessuno = BTreeSet::new();
    let ricordati = match &ricordo {
        Ricordo::Chiavi(chiavi) => chiavi,
        Ricordo::Nessuno | Ricordo::Illeggibile => &nessuno,
    };

    // Da non seppellire: quelli che qui non ci sono — né come file di qui né,
    // per le due regole di [`brani_assenti`], come riga di un altro dispositivo
    // che è la sola a nominarli —, quelli che erano assenti all'ultimo
    // allineamento e adesso ci sono (da agganciare), e — se la memoria non si
    // legge — tutti.
    let da_lasciare = |brano: &str| {
        illeggibile
            || assenti.senza_riga.contains(brano)
            || assenti.solo_specchio.contains(brano)
            || ricordati.contains(brano)
    };
    // Visibile è ciò a cui si può agganciare una voce, e una riga specchio è
    // una riga: la differenza con `da_lasciare` è tutta qui.
    let visibile = |brano: &&str| !assenti.senza_riga.contains(*brano);

    let mut rifatte = 0;
    let mut agganciate = 0;
    for (chiave, appartenenza) in &membri {
        let voluto: Vec<String> = appartenenza
            .brani
            .iter()
            .map(|(_, brano)| brano.clone())
            .collect();
        let atteso: Vec<&str> = voluto.iter().map(String::as_str).collect();
        let sequenza = sequenze.remove(chiave).unwrap_or_else(Sequenza::vuota);
        let mut aggiornata = sequenza.clone();
        if aggiornata.e_vuota() {
            // Il primo battesimo. Vedi [`ORIGINE`] sul perché non porta il nome
            // di questo dispositivo.
            aggiornata = Sequenza::dalla_lista(ORIGINE, &voluto);
        } else {
            // Un brano tornato con il suo file, rimesso in `playlist_tracks` dal
            // trigger della 28 mentre la sequenza ne ha solo lapidi, non è
            // un'aggiunta: vedi [`senza_i_ritorni_di_voci_tolte`].
            let tagliato = if illeggibile {
                None
            } else {
                senza_i_ritorni_di_voci_tolte(&voluto, &sequenza, ricordati)
            };
            let desiderato: &[String] = tagliato.as_deref().unwrap_or(&voluto);
            let qui: Vec<&str> = aggiornata.ordine().into_iter().filter(visibile).collect();
            if !qui
                .iter()
                .copied()
                .eq(desiderato.iter().map(String::as_str))
            {
                aggiornata.riordina_lasciando(dispositivo, desiderato, adesso_ms, da_lasciare);
            }
        }
        if aggiornata != sequenza {
            scrivi_sequenza(tx, chiave, &aggiornata)?;
            rifatte += 1;
        }

        // Quel che resta in più rispetto a `playlist_tracks` sono le voci dei
        // brani arrivati: la sequenza li ha già al loro posto, e qui entrano.
        // Solo se c'è qualcosa di ricordato (o se il ricordo non si legge, e
        // allora si riaggancia tutto): senza, la differenza non può essere
        // altro che un guasto, e `playlist_tracks` non si riscrive per un
        // sospetto.
        if ricordati.is_empty() && !illeggibile {
            continue;
        }
        let finale: Vec<&str> = aggiornata.ordine().into_iter().filter(visibile).collect();
        if finale != atteso {
            riscrivi_appartenenza(tx, appartenenza, &finale)?;
            agganciate += 1;
        }
    }

    // Le sequenze rimaste sono di playlist che non ci sono più (o che sono
    // diventate automatiche): `membri` comprende anche le playlist vuote, quindi
    // una playlist svuotata a mano è già passata di sopra. Vanno sepolti tutti
    // gli elementi, quelli dei brani che qui mancano compresi (una playlist
    // cancellata qui è una decisione, a differenza di un brano che non c'è), o
    // al prossimo giro tornerebbero da soli.
    for (chiave, sequenza) in sequenze {
        if sequenza.ordine().is_empty() {
            continue;
        }
        let mut vuotata = sequenza.clone();
        // Svuotare non crea elementi, solo lapidi: il nome qui è indifferente.
        vuotata.riordina(dispositivo, &[], adesso_ms);
        scrivi_sequenza(tx, &chiave, &vuotata)?;
        rifatte += 1;
    }

    // La memoria si riscrive qui, con `assenti` e `ricordo` che servivano già a
    // decidere: rifarli da capo costava un'interrogazione sulle sequenze e una
    // lettura in più, a ogni passata. Valgono ancora anche dopo aver sepolto le
    // sequenze rimaste, perché `assenti` nomina i brani di **tutti** gli
    // elementi, vivi o tolti, e una lapide non ne cambia la chiave.
    scrivi_il_ricordo(tx, &assenti.senza_riga, &ricordo)?;
    Ok((rifatte, agganciate))
}

/// Toglie dall'elenco voluto le righe di un brano **tornato** che la sequenza
/// non giustifica.
///
/// `ricordati` sono le chiavi che all'ultima passata qui mancavano; se una di
/// loro sta in `playlist_tracks` adesso, il file è tornato e a rimettercela è
/// stato il trigger della migrazione 28, che ridà a un brano il posto che aveva
/// quando è uscito dalla libreria. Se la sequenza di questa playlist per quel
/// brano ha ancora una voce viva, la riga è quella voce e va bene. Se ha solo
/// lapidi, la voce è stata tolta altrove mentre il file non c'era: è il ritorno
/// di una voce tolta, non un'aggiunta, e tenerla come tale ne farebbe un
/// elemento nuovo che la sincronia pubblica — la rimozione vera, fatta
/// sull'altro dispositivo, risorgerebbe ovunque. Per ogni brano tornato **che
/// questa sequenza ha avuto e ha tolto** valgono al più tante righe quante sono
/// le sue voci vive: le ultime sono quelle che si tolgono.
///
/// # Il rischio inverso, scelto e scritto
///
/// Un'aggiunta vera dell'utente non si può distinguere, nelle tabelle, dalla
/// riga che ha rimesso il trigger: stessa tabella, stessa forma. Si limita il
/// danno in due modi.
///
/// - Solo per i brani **tornati** (la chiave era assente all'ultima passata) e
///   solo nella passata in cui tornano: da quella dopo il brano non è più nella
///   memoria, e una riga messa dall'utente è una voce nuova come qualunque altra.
/// - Solo dove la sequenza **ha lapidi per quel brano**. Se non ne ha — la
///   playlist non ha mai nominato il brano, o lo nomina vivo — la riga è
///   un'aggiunta e si lascia: è il caso comune, e il gesto «aggiungi alla
///   playlist» un brano appena arrivato non si perde.
///
/// Quel che resta è un'aggiunta fatta in una finestra di pochi minuti, di un
/// brano che era stato in quella playlist, tolto altrove, e che è appena
/// tornato: viene tolta anche lei e va rifatta. È l'errore sicuro: un gesto da
/// ripetere costa un clic, una rimozione che risorge su tutti i dispositivi no.
///
/// `None` se non c'è niente da togliere (quasi sempre).
fn senza_i_ritorni_di_voci_tolte(
    voluto: &[String],
    sequenza: &Sequenza,
    ricordati: &BTreeSet<String>,
) -> Option<Vec<String>> {
    if !voluto.iter().any(|brano| ricordati.contains(brano)) {
        return None;
    }
    // Per ogni brano tornato: quante voci vive ha questa sequenza, e se ha
    // anche delle lapidi.
    let mut vive: BTreeMap<&str, usize> = BTreeMap::new();
    let mut con_lapidi: BTreeSet<&str> = BTreeSet::new();
    for (_, elemento) in sequenza.elementi() {
        let brano = elemento.brano.as_str();
        if !ricordati.contains(brano) {
            continue;
        }
        if elemento.tolto.is_some() {
            con_lapidi.insert(brano);
        } else {
            *vive.entry(brano).or_insert(0) += 1;
        }
    }
    let mut viste: BTreeMap<&str, usize> = BTreeMap::new();
    let mut tolto_qualcosa = false;
    let tenute: Vec<String> = voluto
        .iter()
        .filter(|brano| {
            if !con_lapidi.contains(brano.as_str()) {
                return true;
            }
            let viste_finora = viste.entry(brano.as_str()).or_insert(0);
            *viste_finora += 1;
            let tenuta = *viste_finora <= vive.get(brano.as_str()).copied().unwrap_or(0);
            tolto_qualcosa |= !tenuta;
            tenuta
        })
        .cloned()
        .collect();
    tolto_qualcosa.then_some(tenute)
}

/// Le chiavi dei brani che qualche voce di una sequenza nomina e che questa
/// libreria non ha.
///
/// **Vive o tolte**: una voce tolta altrove mentre il file non c'era continua a
/// nominare il brano, e il brano continua a mancare. Così la memoria di
/// [`CHIAVE_BRANI_ASSENTI`] è «appiccicosa» — una chiave ne esce solo quando il
/// suo brano è tornato — e la passata che vede tornare un file sa che era
/// assente anche se la voce, nel frattempo, è stata sepolta (vedi
/// [`senza_i_ritorni_di_voci_tolte`]). Se la limitassimo alle voci vive, la
/// lapide arrivata dall'altro dispositivo la farebbe uscire prima del ritorno.
///
/// Su tutte le sequenze insieme e non playlist per playlist: è
/// un'interrogazione sola, e usa `idx_tracks_track_key`.
///
/// # Due regole, e perché non sono la stessa
///
/// Una riga specchio (il brano di un altro dispositivo, `lontano://…`) risponde
/// a due domande diverse, e la risposta è opposta.
///
/// - **Per agganciare** — una voce viva entra in `playlist_tracks` su una riga —
///   una riga specchio **conta** come presente: senza, sul telefono le playlist
///   perderebbero i brani del PC. Vale per [`Assenti::senza_riga`], che è la
///   memoria di [`CHIAVE_BRANI_ASSENTI`] e il filtro di ciò che è visibile.
/// - **Per seppellire** — una voce viva che manca in `playlist_tracks` diventa
///   «tolta», e la lapide viaggia — una chiave presente qui **solo attraverso
///   righe specchio** conta come **assente**: [`Assenti::solo_specchio`]. Una
///   riga specchio nasce, cambia chiave e sparisce per volontà del proprietario
///   e della rete; che la sua chiave resti per un'altra riga (un tag corretto
///   qui sulla copia di qui, che ha ancora la vecchia chiave sul PC) non dice
///   niente di cosa l'utente ha fatto della playlist. Il solo gesto che toglie
///   una voce così è dell'utente, e la sua lapide la scrive il gesto
///   ([`seppellisci_le_voci_tolte`]).
///
/// Le due insieme in una lettura sola: una chiave che qui ha almeno una riga di
/// questa macchina non c'è in nessuno dei due elenchi.
struct Assenti {
    /// Nessuna riga, né di qui né specchio: la voce non si può agganciare a
    /// niente.
    senza_riga: BTreeSet<String>,
    /// Righe sì, ma tutte specchio: la voce si aggancia, ma non si seppellisce
    /// per differenza.
    solo_specchio: BTreeSet<String>,
}

fn brani_assenti(connection: &Connection) -> Result<Assenti, AppError> {
    #[cfg(test)]
    LETTURE.with(|letture| letture.set((letture.get().0 + 1, letture.get().1)));
    let mut statement = connection
        .prepare(
            "SELECT DISTINCT s.track_key,
                    EXISTS (SELECT 1 FROM tracks m WHERE m.track_key = s.track_key)
               FROM sync_sequenza s
              WHERE NOT EXISTS (SELECT 1 FROM tracks t
                                 WHERE t.track_key = s.track_key
                                   AND (t.path IS NULL OR t.path < 'lontano://'
                                        OR t.path >= 'lontano:/0'))",
        )
        .map_err(|err| db_error("brani delle playlist che qui mancano", &err))?;
    let chiavi = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)))
        .map_err(|err| db_error("brani delle playlist che qui mancano", &err))?;
    let mut assenti = Assenti {
        senza_riga: BTreeSet::new(),
        solo_specchio: BTreeSet::new(),
    };
    for riga in chiavi {
        let (chiave, ha_una_riga) =
            riga.map_err(|err| db_error("brani delle playlist che qui mancano", &err))?;
        if ha_una_riga {
            assenti.solo_specchio.insert(chiave);
        } else {
            assenti.senza_riga.insert(chiave);
        }
    }
    Ok(assenti)
}

/// Come si presenta la memoria dei brani assenti quando la si legge: vedi
/// [`CHIAVE_BRANI_ASSENTI`].
#[derive(Debug, PartialEq, Eq)]
enum Ricordo {
    /// Non c'è: nessun ricordo. È lo stato normale di chi ha tutta la musica.
    Nessuno,
    /// C'è un valore e non si legge. Non è «niente è arrivato»: non si sa, e in
    /// quella passata non si seppellisce niente.
    Illeggibile,
    /// Le chiavi che l'ultimo allineamento ha trovato assenti.
    Chiavi(BTreeSet<String>),
}

/// Legge la memoria dei brani assenti, distinguendo «manca» da «non si legge».
///
/// `settings::read_json` non lo permette: un JSON storto vale per lei come un
/// valore assente, ed è proprio quel che qui non si può fare.
fn ricordo_dei_brani_assenti(connection: &Connection) -> Result<Ricordo, AppError> {
    #[cfg(test)]
    LETTURE.with(|letture| letture.set((letture.get().0, letture.get().1 + 1)));
    let Some(grezzo) = settings::read(connection, CHIAVE_BRANI_ASSENTI)? else {
        return Ok(Ricordo::Nessuno);
    };
    Ok(serde_json::from_str(&grezzo).map_or(Ricordo::Illeggibile, Ricordo::Chiavi))
}

/// Ricorda quali brani le sequenze nominano e qui non ci sono, adesso.
///
/// Da chiamare dopo ogni scrittura di `sync_sequenza` e dopo ogni confronto fra
/// lei e `tracks`: [`allinea_in`] e [`applica_in`]. Non scrive se la memoria è
/// già quella (la libreria di chi ha tutta la musica non ne ha mai una), e la
/// toglie quando non c'è più niente da ricordare. Una memoria illeggibile si
/// rifà sempre, anche se quella di adesso è vuota: lasciarla lì vorrebbe dire
/// che la passata dopo la ritrova guasta, e non seppellisce niente neanche lei.
pub(crate) fn ricorda_i_brani_assenti(connection: &Connection) -> Result<(), AppError> {
    let adesso = brani_assenti(connection)?.senza_riga;
    let ricordo = ricordo_dei_brani_assenti(connection)?;
    scrivi_il_ricordo(connection, &adesso, &ricordo)
}

/// Il corpo di [`ricorda_i_brani_assenti`], per chi ha già letto i brani assenti
/// e la memoria com'era: [`allinea_sequenze`] li ha in mano per decidere, e non
/// li rilegge.
fn scrivi_il_ricordo(
    connection: &Connection,
    adesso: &BTreeSet<String>,
    ricordo: &Ricordo,
) -> Result<(), AppError> {
    match ricordo {
        Ricordo::Nessuno if adesso.is_empty() => return Ok(()),
        Ricordo::Chiavi(ricordati) if ricordati == adesso => return Ok(()),
        Ricordo::Nessuno | Ricordo::Illeggibile | Ricordo::Chiavi(_) => {}
    }
    if adesso.is_empty() {
        settings::forget(connection, CHIAVE_BRANI_ASSENTI)?;
        return Ok(());
    }
    settings::write_json(connection, CHIAVE_BRANI_ASSENTI, adesso)
}

// Quante volte questo filo ha interrogato i brani assenti e letto la memoria:
// la prova del costo di una passata ferma. Solo nei test.
#[cfg(test)]
thread_local! {
    static LETTURE: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

/// Riscrive l'appartenenza di una playlist con questo ordine.
///
/// Le righe che ci sono già mantengono il brano che nominano: il file è quello
/// che l'utente vede, e due file con la stessa chiave non si scambiano di posto
/// per averne cercato uno per chiave (`applica_playlist` lo fa, perché lì la
/// playlist si rifà da una sequenza). Chi non ha una riga la prende da
/// `tracks`. Si svuota e si riscrive come là: la chiave `(playlist_id,
/// position)` si verifica a ogni istruzione, e le posizioni escono `0..n`.
fn riscrivi_appartenenza(
    tx: &Transaction<'_>,
    appartenenza: &Appartenenza,
    ordine: &[&str],
) -> Result<(), AppError> {
    let mut gia: BTreeMap<&str, VecDeque<i64>> = BTreeMap::new();
    for (track_id, brano) in &appartenenza.brani {
        gia.entry(brano.as_str()).or_default().push_back(*track_id);
    }
    let mut cerca = tx
        .prepare("SELECT id FROM tracks WHERE track_key = ?1 ORDER BY (path >= 'lontano://' AND path < 'lontano:/0'), id LIMIT 1")
        .map_err(|err| db_error("brani da agganciare", &err))?;
    let mut brani = Vec::with_capacity(ordine.len());
    for brano in ordine {
        let riusato = gia.get_mut(brano).and_then(VecDeque::pop_front);
        let track_id = match riusato {
            Some(track_id) => track_id,
            None => match cerca.query_row([brano], |row| row.get::<_, i64>(0)) {
                Ok(track_id) => track_id,
                // Sparito fra la lettura e la scrittura: nella stessa transazione
                // non succede, ma se succede il posto si salta, non si inventa.
                Err(rusqlite::Error::QueryReturnedNoRows) => continue,
                Err(err) => return Err(db_error("brani da agganciare", &err)),
            },
        };
        brani.push(track_id);
    }

    tx.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
        [appartenenza.id],
    )
    .map_err(|err| db_error("brani da agganciare", &err))?;
    let mut accoda = tx
        .prepare(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        )
        .map_err(|err| db_error("brani da agganciare", &err))?;
    for (posizione, track_id) in brani.iter().enumerate() {
        let posizione = i64::try_from(posizione).unwrap_or(i64::MAX);
        accoda
            .execute(rusqlite::params![appartenenza.id, track_id, posizione])
            .map_err(|err| db_error("brani da agganciare", &err))?;
    }
    Ok(())
}

/// Il nome con cui si battezzano gli elementi che si aggiungono qui a una
/// sequenza già esistente: quello del dispositivo.
///
/// Un nome vuoto farebbe identità che `Identita::da_testo` non rilegge, e il
/// brano aggiunto sparirebbe alla lettura dopo: meglio il nome comune.
pub(crate) fn nome_delle_aggiunte(dispositivo: &str) -> &str {
    if dispositivo.trim().is_empty() {
        ORIGINE
    } else {
        dispositivo
    }
}

/// Le cartelle sorvegliate come mappa datata, in pari con l'elenco.
///
/// L'elenco è la verità su questo computer; la mappa è ciò che di quella verità
/// si può raccontare a un altro dispositivo. Una cartella mai vista prima entra
/// con data **zero** e non con l'ora attuale, ed è la differenza fra una
/// sincronia che funziona e una che rimette le cartelle appena tolte: al primo
/// avvio nessuna delle cartelle già configurate è una *decisione* presa adesso, e
/// datarle adesso le farebbe vincere su una rimozione fatta ieri sul portatile.
fn allinea_cartelle(tx: &Transaction<'_>, adesso_ms: i64) -> Result<usize, AppError> {
    allinea_mappa_delle_cartelle(tx, CHIAVE_CARTELLE_SINCRONIA, adesso_ms)
}

/// Il corpo di [`allinea_cartelle`], per una mappa qualunque.
///
/// Il backup ne tiene una sua (`backup::allinea`) con la stessa regola: se
/// scrivesse in quella della sincronia anche a sincronia spenta, le cartelle
/// tolte prima di accenderla arriverebbero agli altri dispositivi come
/// decisioni datate, e li spegnerebbero.
pub(crate) fn allinea_mappa_delle_cartelle(
    tx: &Transaction<'_>,
    chiave_mappa: &str,
    adesso_ms: i64,
) -> Result<usize, AppError> {
    let elenco: Vec<String> = settings::read_json(tx, CHIAVE_CARTELLE)?.unwrap_or_default();
    let mut mappa: BTreeMap<String, Interruttore> =
        settings::read_json(tx, chiave_mappa)?.unwrap_or_default();
    let prima_volta = mappa.is_empty();
    let presenti: BTreeSet<&str> = elenco.iter().map(String::as_str).collect();

    let mut cambiate = 0;
    for percorso in &elenco {
        match mappa.get(percorso) {
            Some(stato) if stato.on => {}
            Some(_) => {
                mappa.insert(
                    percorso.clone(),
                    Interruttore {
                        on: true,
                        at: adesso_ms,
                    },
                );
                cambiate += 1;
            }
            None => {
                mappa.insert(
                    percorso.clone(),
                    Interruttore {
                        on: true,
                        at: if prima_volta { 0 } else { adesso_ms },
                    },
                );
                cambiate += 1;
            }
        }
    }
    for (percorso, stato) in &mut mappa {
        if stato.on && !presenti.contains(percorso.as_str()) {
            *stato = Interruttore {
                on: false,
                at: adesso_ms,
            };
            cambiate += 1;
        }
    }

    if cambiate > 0 {
        settings::write_json(tx, chiave_mappa, &mappa)?;
    }
    Ok(cambiate)
}

// ── dal database al documento ───────────────────────────────────────────────

/// Costruisce il documento di questo dispositivo.
///
/// `skin` e `bozze` arrivano da fuori, come già fa `backup::snapshot`: i
/// pacchetti stanno su disco e questo crate non li apre.
///
/// # Cosa non entra nel documento
///
/// I brani **senza niente da dire**. Un brano mai ascoltato, senza voto e senza
/// preferito non porta informazione che una fusione possa usare, e su una
/// libreria appena scansionata sono la quasi totalità: è la differenza fra un
/// documento da settanta chilobyte e uno da mezzo megabyte, riscritto a ogni
/// passata.
///
/// # Errori
///
/// `db.queryFailed` se una lettura fallisce.
pub fn contenuto(
    connection: &Connection,
    dispositivo: &str,
    skin: BTreeMap<String, String>,
    bozze: BTreeMap<String, String>,
) -> Result<Contenuto, AppError> {
    Ok(Contenuto {
        ascolti: ascolti_di(connection, dispositivo)?,
        ereditati: ascolti_di(connection, IMPORTAZIONE)?,
        ultimo: ultimi_ascolti(connection)?,
        voti: voti(connection)?,
        preferiti: preferiti(connection)?,
        posizioni: posizioni(connection)?,
        playlist: playlist(connection)?,
        lapidi: lapidi(connection)?,
        cartelle: settings::read_json(connection, CHIAVE_CARTELLE_SINCRONIA)?.unwrap_or_default(),
        skin,
        bozze,
        skin_attiva: settings::read(connection, CHIAVE_SKIN)?.map(|id| Scelta {
            id,
            at: settings::read(connection, CHIAVE_SKIN_QUANDO)
                .ok()
                .flatten()
                .and_then(|quando| quando.parse().ok())
                .unwrap_or(0),
        }),
    })
}

/// Gli ascolti attribuiti a un dispositivo.
fn ascolti_di(
    connection: &Connection,
    dispositivo: &str,
) -> Result<BTreeMap<String, i64>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key, quanti FROM sync_ascolti WHERE dispositivo = ?1 AND quanti > 0")
        .map_err(|err| db_error("ascolti da pubblicare", &err))?;
    let righe = statement
        .query_map([dispositivo], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| db_error("ascolti da pubblicare", &err))?;
    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, quanti) = riga.map_err(|err| db_error("ascolti da pubblicare", &err))?;
        mappa.insert(chiave, quanti);
    }
    Ok(mappa)
}

/// L'ultimo ascolto di ciascun brano.
fn ultimi_ascolti(connection: &Connection) -> Result<BTreeMap<String, i64>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT track_key, MAX(last_played_at) FROM tracks
              WHERE last_played_at IS NOT NULL AND NOT (path >= 'lontano://' AND path < 'lontano:/0') GROUP BY track_key",
        )
        .map_err(|err| db_error("ultimi ascolti", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| db_error("ultimi ascolti", &err))?;
    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, quando) = riga.map_err(|err| db_error("ultimi ascolti", &err))?;
        mappa.insert(chiave, quando);
    }
    Ok(mappa)
}

/// I voti, con la data della decisione.
///
/// Solo le righe con `rating_at`: una riga senza è un brano su cui **nessuno ha
/// mai deciso niente**, ed è diverso da uno votato zero. Spedire uno zero non
/// datato come se fosse una decisione toglierebbe le stelle sull'altro
/// dispositivo, che è esattamente il guasto che `merge_rating` evita trattando
/// lo zero come assenza.
fn voti(connection: &Connection) -> Result<BTreeMap<String, Voto>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT track_key, rating, rating_at FROM tracks
              WHERE rating_at IS NOT NULL AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
        )
        .map_err(|err| db_error("voti da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| db_error("voti da pubblicare", &err))?;
    let mut mappa: BTreeMap<String, Voto> = BTreeMap::new();
    for riga in righe {
        let (chiave, stelle, quando) = riga.map_err(|err| db_error("voti da pubblicare", &err))?;
        let voto = Voto {
            v: u8::try_from(stelle.clamp(0, 5)).unwrap_or(0),
            at: quando,
        };
        // Due file dello stesso brano: vince la decisione più recente, che è la
        // stessa regola con cui i due si fonderebbero fra dispositivi.
        mappa
            .entry(chiave)
            .and_modify(|gia| {
                if voto.at > gia.at {
                    *gia = voto;
                }
            })
            .or_insert(voto);
    }
    Ok(mappa)
}

/// I preferiti, messi e tolti.
///
/// `liked_at` c'è dalla migrazione 1 e data la **decisione**: è la ragione per
/// cui un «non mi piace più» viaggia già oggi invece di essere riassorbito dal
/// cuoricino più vecchio dell'altro dispositivo.
fn preferiti(connection: &Connection) -> Result<BTreeMap<String, Interruttore>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT track_key, liked, liked_at FROM tracks
              WHERE liked_at IS NOT NULL AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
        )
        .map_err(|err| db_error("preferiti da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| db_error("preferiti da pubblicare", &err))?;
    let mut mappa: BTreeMap<String, Interruttore> = BTreeMap::new();
    for riga in righe {
        let (chiave, acceso, quando) =
            riga.map_err(|err| db_error("preferiti da pubblicare", &err))?;
        let stato = Interruttore {
            on: acceso != 0,
            at: quando,
        };
        mappa
            .entry(chiave)
            .and_modify(|gia| {
                if stato.at > gia.at {
                    *gia = stato;
                }
            })
            .or_insert(stato);
    }
    Ok(mappa)
}

/// Dove si era arrivati nei brani lasciati a metà.
fn posizioni(connection: &Connection) -> Result<BTreeMap<String, Momento>, AppError> {
    let mut statement = connection
        .prepare("SELECT track_key, ms, at_ms FROM sync_posizioni")
        .map_err(|err| db_error("posizioni da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Momento {
                    ms: row.get(1)?,
                    at: row.get(2)?,
                },
            ))
        })
        .map_err(|err| db_error("posizioni da pubblicare", &err))?;
    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, momento) = riga.map_err(|err| db_error("posizioni da pubblicare", &err))?;
        mappa.insert(chiave, momento);
    }
    Ok(mappa)
}

/// Le playlist con la loro sequenza.
fn playlist(connection: &Connection) -> Result<BTreeMap<String, PlaylistSincronizzata>, AppError> {
    let mut sequenze = sequenze_salvate(connection)?;
    let mut statement = connection
        .prepare(
            "SELECT playlist_key, name, description, created_at, updated_at, is_smart, rules
               FROM playlists",
        )
        .map_err(|err| db_error("playlist da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            let smart: i64 = row.get(5)?;
            Ok((
                row.get::<_, String>(0)?,
                PlaylistSincronizzata {
                    nome: row.get(1)?,
                    descrizione: row.get(2)?,
                    creata_at: row.get(3)?,
                    at: row.get(4)?,
                    smart: smart != 0,
                    regole: row.get(6)?,
                    sequenza: Sequenza::vuota(),
                },
            ))
        })
        .map_err(|err| db_error("playlist da pubblicare", &err))?;

    let mut mappa = BTreeMap::new();
    for riga in righe {
        let (chiave, mut playlist) =
            riga.map_err(|err| db_error("playlist da pubblicare", &err))?;
        // Le automatiche non portano appartenenza: ogni dispositivo la ricalcola
        // dalle regole, così resta vera anche sui brani che l'altro non ha.
        if !playlist.smart {
            playlist.sequenza = sequenze.remove(&chiave).unwrap_or_else(Sequenza::vuota);
        }
        mappa.insert(chiave, playlist);
    }
    Ok(mappa)
}

/// Le cancellazioni registrate qui.
fn lapidi(connection: &Connection) -> Result<Lapidi, AppError> {
    let mut statement = connection
        .prepare("SELECT kind, key, deleted_at FROM sync_tombstones")
        .map_err(|err| db_error("lapidi da pubblicare", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|err| db_error("lapidi da pubblicare", &err))?;
    let mut lapidi = Lapidi::default();
    for riga in righe {
        let (tipo, chiave, quando) = riga.map_err(|err| db_error("lapidi da pubblicare", &err))?;
        match tipo.as_str() {
            "track" => {
                lapidi.brani.insert(chiave, quando);
            }
            "playlist" => {
                lapidi.playlist.insert(chiave, quando);
            }
            _ => {}
        }
    }
    Ok(lapidi)
}

// ── le sequenze su disco ────────────────────────────────────────────────────

/// Una playlist non automatica com'è in `playlist_tracks`.
struct Appartenenza {
    /// L'identificativo della riga di `playlists`.
    id: i64,
    /// Le righe, nell'ordine: l'identificativo del brano e la sua chiave.
    brani: Vec<(i64, String)>,
}

/// I membri di ogni playlist non automatica, nell'ordine.
///
/// Una interrogazione per tutte e non una per ciascuna: su quaranta playlist la
/// differenza fra le due forme è quaranta andate e ritorno, e questo codice gira
/// con il lucchetto della libreria in mano.
///
/// # Il `JOIN` su `tracks` dice cosa vuol dire «membro»
///
/// Un membro è un brano **che questa libreria ha**: `playlist_tracks` punta a
/// `tracks(id)` e un brano senza riga non può starci. Quel che la sequenza
/// nomina e qui manca non è qui un membro, ma non per questo è stato tolto: lo
/// dice [`allinea_sequenze`], che confronta questo elenco soltanto con le voci
/// dei brani che qui ci sono.
fn membri_per_playlist(
    connection: &Connection,
) -> Result<BTreeMap<String, Appartenenza>, AppError> {
    // Prima tutte le playlist, anche le vuote: senza, una playlist svuotata non
    // verrebbe mai confrontata e i suoi elementi resterebbero vivi nella
    // sequenza.
    let mut mappa: BTreeMap<String, Appartenenza> = BTreeMap::new();
    let mut tutte = connection
        .prepare("SELECT playlist_key, id FROM playlists WHERE is_smart = 0")
        .map_err(|err| db_error("playlist", &err))?;
    let righe = tutte
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| db_error("playlist", &err))?;
    for riga in righe {
        let (chiave, id) = riga.map_err(|err| db_error("playlist", &err))?;
        mappa.insert(
            chiave,
            Appartenenza {
                id,
                brani: Vec::new(),
            },
        );
    }

    let mut statement = connection
        .prepare(
            "SELECT p.playlist_key, t.id, t.track_key
               FROM playlist_tracks pt
               JOIN playlists p ON p.id = pt.playlist_id
               JOIN tracks t    ON t.id = pt.track_id
              WHERE p.is_smart = 0
              ORDER BY p.playlist_key, pt.position",
        )
        .map_err(|err| db_error("membri delle playlist", &err))?;
    let righe = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| db_error("membri delle playlist", &err))?;
    for riga in righe {
        let (playlist, track_id, brano) =
            riga.map_err(|err| db_error("membri delle playlist", &err))?;
        if let Some(appartenenza) = mappa.get_mut(&playlist) {
            appartenenza.brani.push((track_id, brano));
        }
    }
    Ok(mappa)
}

/// Le sequenze come stanno scritte.
fn sequenze_salvate(connection: &Connection) -> Result<BTreeMap<String, Sequenza>, AppError> {
    leggi_le_sequenze(
        connection,
        "SELECT playlist_key, elemento, dopo, track_key, tolto_ms
           FROM sync_sequenza ORDER BY playlist_key",
        [],
    )
}

/// La sequenza di una playlist sola, com'è scritta: vuota se non ce n'è.
fn sequenza_salvata(connection: &Connection, playlist_key: &str) -> Result<Sequenza, AppError> {
    Ok(leggi_le_sequenze(
        connection,
        "SELECT playlist_key, elemento, dopo, track_key, tolto_ms
           FROM sync_sequenza WHERE playlist_key = ?1",
        [playlist_key],
    )?
    .remove(playlist_key)
    .unwrap_or_else(Sequenza::vuota))
}

/// Il corpo di [`sequenze_salvate`] e [`sequenza_salvata`]: la stessa lettura,
/// con la riga d'identità illeggibile saltata allo stesso modo.
fn leggi_le_sequenze<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    parametri: P,
) -> Result<BTreeMap<String, Sequenza>, AppError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|err| db_error("sequenze salvate", &err))?;
    let righe = statement
        .query_map(parametri, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
            ))
        })
        .map_err(|err| db_error("sequenze salvate", &err))?;

    let mut per_playlist: BTreeMap<String, Vec<(Identita, Elemento)>> = BTreeMap::new();
    for riga in righe {
        let (playlist, elemento, dopo, brano, tolto) =
            riga.map_err(|err| db_error("sequenze salvate", &err))?;
        // Un'identità illeggibile fa sparire **quell'elemento**, non la playlist:
        // è la stessa indulgenza con cui `interpreta` legge un documento remoto,
        // e per la stessa ragione — perdere un brano è meglio che perdere l'ordine
        // di tutti gli altri.
        let Some(identita) = Identita::da_testo(&elemento) else {
            continue;
        };
        per_playlist.entry(playlist).or_default().push((
            identita,
            Elemento {
                dopo: dopo.as_deref().and_then(Identita::da_testo),
                brano,
                tolto,
            },
        ));
    }
    Ok(per_playlist
        .into_iter()
        .map(|(chiave, elementi)| (chiave, Sequenza::dagli_elementi(elementi)))
        .collect())
}

/// Riscrive la sequenza di una playlist.
fn scrivi_sequenza(
    tx: &Connection,
    playlist_key: &str,
    sequenza: &Sequenza,
) -> Result<(), AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_sequenza (playlist_key, elemento, dopo, track_key, tolto_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(playlist_key, elemento) DO UPDATE SET
               dopo      = excluded.dopo,
               track_key = excluded.track_key,
               -- La lapide più vecchia vince: il momento in cui una cosa è stata
               -- tolta la prima volta è un fatto, non un'opinione. È la stessa
               -- regola che `Sequenza::fondi` applica in memoria.
               tolto_ms  = CASE
                             WHEN sync_sequenza.tolto_ms IS NULL THEN excluded.tolto_ms
                             WHEN excluded.tolto_ms IS NULL THEN sync_sequenza.tolto_ms
                             ELSE min(sync_sequenza.tolto_ms, excluded.tolto_ms)
                           END",
        )
        .map_err(|err| db_error("sequenza", &err))?;
    for (identita, elemento) in sequenza.elementi() {
        scrivi
            .execute(rusqlite::params![
                playlist_key,
                identita.testo(),
                elemento.dopo.as_ref().map(Identita::testo),
                elemento.brano,
                elemento.tolto,
            ])
            .map_err(|err| db_error("sequenza", &err))?;
    }
    Ok(())
}

/// Fa seguire la sequenza a una playlist che cambia chiave.
///
/// La chiave **è** il nome, e un rinomino la cambia. La sequenza sta sotto la
/// chiave: se non la si sposta, la playlist col nome nuovo non ne ha una, e
/// `allinea_sequenze` la battezza da zero con i soli brani che qui ci sono;
/// quella sotto il nome vecchio, che non appartiene più a nessuna playlist,
/// viene sepolta intera. Su una libreria completa si vede poco, ma su un
/// telefono con una parte della musica la voce di ogni brano che qui manca
/// muore con il nome vecchio, e gli altri dispositivi — per i quali la
/// playlist col nome nuovo nasce senza quel brano — lo perdono (FSYNC2).
///
/// Va chiamata **dopo** che l'`UPDATE` della chiave è andato a buon fine, nella
/// stessa transazione, con la chiave di prima e quella di adesso.
///
/// # Perché si svuota prima la chiave nuova, e non si fonde
///
/// La chiave è unica: una riga di sequenza sotto la chiave nuova può essere
/// solo il resto di una playlist con quel nome **cancellata qui**. Le sue
/// identità (`origine:0`…) sono quelle di qualunque sequenza battezzata da una
/// lista, e fondere significherebbe stampare le lapidi di una playlist che non
/// c'è più sulle voci vive di quella che arriva. Si butta il resto.
///
/// Restituisce quante righe di sequenza si sono spostate.
///
/// # Errori
///
/// `db.queryFailed` se una scrittura fallisce.
pub(crate) fn sposta_la_sequenza(
    connection: &Connection,
    vecchia: &str,
    nuova: &str,
) -> Result<usize, AppError> {
    if vecchia == nuova {
        return Ok(0);
    }
    connection
        .execute("DELETE FROM sync_sequenza WHERE playlist_key = ?1", [nuova])
        .map_err(|err| db_error("sequenza sotto il nome nuovo di una playlist", &err))?;
    connection
        .execute(
            "UPDATE sync_sequenza SET playlist_key = ?2 WHERE playlist_key = ?1",
            [vecchia, nuova],
        )
        .map_err(|err| db_error("sequenza di una playlist rinominata", &err))
}

/// Seppellisce la sequenza di una playlist che viene cancellata, **subito**.
///
/// Una playlist cancellata e rifatta con lo stesso nome prima di un `allinea`
/// ritrova sotto la sua chiave la sequenza di quella di prima: voci vive
/// comprese, e fra queste quelle dei brani che qui mancano, che
/// `allinea_sequenze` lascia stare e che resterebbero vive nella playlist nuova
/// (FSYNC2). Dall'`allinea` non si può riconoscere che la playlist è un'altra —
/// la lapide della vecchia e la riga della nuova convivono fino alla passata che
/// le applica, e rifarlo a ogni `allinea` seppellirebbe anche le voci appena
/// battezzate di quella nuova, e la passata che le applica cadrebbe — quindi si
/// fa nell'unico posto in cui una playlist si cancella,
/// [`crate::playlists::cancella`], nella sua transazione: è quel che farebbe il
/// primo `allinea` dopo (il ramo delle «sequenze rimaste»), solo prima.
///
/// Restituisce gli elementi che erano vivi, per [`rianima_la_sequenza`]:
/// «Annulla» li deve poter rimettere come stavano.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub(crate) fn seppellisci_la_sequenza(
    connection: &Connection,
    playlist_key: &str,
    adesso_ms: i64,
) -> Result<Vec<String>, AppError> {
    let vive: Vec<String> = {
        let mut statement = connection
            .prepare(
                "SELECT elemento FROM sync_sequenza
                  WHERE playlist_key = ?1 AND tolto_ms IS NULL",
            )
            .map_err(|err| db_error("sequenza di una playlist cancellata", &err))?;
        statement
            .query_map([playlist_key], |row| row.get::<_, String>(0))
            .map_err(|err| db_error("sequenza di una playlist cancellata", &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| db_error("sequenza di una playlist cancellata", &err))?
    };
    connection
        .execute(
            "UPDATE sync_sequenza SET tolto_ms = ?2
              WHERE playlist_key = ?1 AND tolto_ms IS NULL",
            rusqlite::params![playlist_key, adesso_ms],
        )
        .map_err(|err| db_error("sequenza di una playlist cancellata", &err))?;
    Ok(vive)
}

/// Rimette vive le voci che [`seppellisci_la_sequenza`] aveva sepolto, se sono
/// ancora com'erano: «Annulla» una cancellazione.
///
/// Solo gli elementi nominati **e** con la data di sepoltura scritta da lei: una
/// voce che nel frattempo è stata sepolta per un'altra ragione ha un'altra data,
/// e non si tocca.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub(crate) fn rianima_la_sequenza(
    connection: &Connection,
    playlist_key: &str,
    elementi: &[String],
    sepolti_il: i64,
) -> Result<(), AppError> {
    let mut statement = connection
        .prepare(
            "UPDATE sync_sequenza SET tolto_ms = NULL
              WHERE playlist_key = ?1 AND elemento = ?2 AND tolto_ms = ?3",
        )
        .map_err(|err| db_error("sequenza di una playlist ripristinata", &err))?;
    for elemento in elementi {
        statement
            .execute(rusqlite::params![playlist_key, elemento, sepolti_il])
            .map_err(|err| db_error("sequenza di una playlist ripristinata", &err))?;
    }
    Ok(())
}

// ── la lapide esplicita ─────────────────────────────────────────────────────

/// Le chiavi delle voci di una playlist il cui brano qui c'è **solo** come riga
/// specchio, nell'ordine in cui stanno. Da leggere **prima** di un gesto che
/// toglie voci: vedi [`seppellisci_le_voci_tolte`].
///
/// Chi non ha righe specchio — il caso comune — non paga che una `EXISTS`.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub(crate) fn voci_solo_specchio(
    connection: &Connection,
    playlist_id: i64,
) -> Result<Vec<String>, AppError> {
    let cosa = "voci di playlist su righe specchio";
    let c_e_uno: bool = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM tracks
                             WHERE path >= 'lontano://' AND path < 'lontano:/0')",
            [],
            |riga| riga.get(0),
        )
        .map_err(|err| db_error(cosa, &err))?;
    if !c_e_uno {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare_cached(
            "SELECT t.track_key
               FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
              WHERE pt.playlist_id = ?1
                AND t.path >= 'lontano://' AND t.path < 'lontano:/0'
                AND NOT EXISTS (SELECT 1 FROM tracks l
                                 WHERE l.track_key = t.track_key
                                   AND (l.path IS NULL OR l.path < 'lontano://'
                                        OR l.path >= 'lontano:/0'))
              ORDER BY pt.position",
        )
        .map_err(|err| db_error(cosa, &err))?;
    statement
        .query_map([playlist_id], |riga| riga.get::<_, String>(0))
        .map_err(|err| db_error(cosa, &err))?
        .collect::<Result<_, _>>()
        .map_err(|err| db_error(cosa, &err))
}

/// La lapide di un gesto dell'utente: le voci che `prima` nominava
/// ([`voci_solo_specchio`], letto prima del gesto) e che la playlist non ha più
/// si seppelliscono nella sua sequenza, **adesso**, e non per differenza al
/// prossimo [`allinea`].
///
/// # Perché esplicita
///
/// Per le voci di un brano che qui c'è solo come riga specchio la differenza
/// non vale più niente: una chiave presente solo attraverso righe specchio
/// conta come assente per la lapide ([`brani_assenti`]), perché una riga
/// specchio che nasce, cambia chiave o sparisce è un evento della rete e non un
/// gesto, e la sua lapide partirebbe verso un dispositivo che il brano ce l'ha.
/// Ma il gesto — «togli dalla playlist», la sostituzione da un'importazione —
/// non può smettere di propagarsi: chi lo compie scrive qui la lapide per quel
/// che ha tolto, e nient'altro.
///
/// Si seppelliscono le **ultime** occorrenze del brano nella sequenza, tante
/// quante voci sono sparite dalla playlist: è quel che farebbe
/// [`Sequenza::riordina_lasciando`], che consuma per prime le occorrenze che la
/// playlist ancora vuole. Una playlist che non ha ancora una sequenza (la
/// sincronia spenta, mai accesa) non ha niente da seppellire, e il primo
/// battesimo nasce già senza quella voce.
///
/// Restituisce quante voci ha sepolto.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub(crate) fn seppellisci_le_voci_tolte(
    connection: &Connection,
    playlist_id: i64,
    prima: &[String],
    adesso_ms: i64,
) -> Result<usize, AppError> {
    if prima.is_empty() {
        return Ok(0);
    }
    let cosa = "lapide di una voce tolta da una playlist";
    let chiave: Option<String> = connection
        .query_row(
            "SELECT playlist_key FROM playlists WHERE id = ?1",
            [playlist_id],
            |riga| riga.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            altro => Err(altro),
        })
        .map_err(|err| db_error(cosa, &err))?;
    let Some(chiave) = chiave else {
        return Ok(0);
    };
    let mut sequenza = sequenza_salvata(connection, &chiave)?;
    if sequenza.e_vuota() {
        return Ok(0);
    }

    // Quante voci per chiave ha adesso la playlist, di qualunque riga.
    let ora: BTreeMap<String, usize> = {
        let mut statement = connection
            .prepare_cached(
                "SELECT t.track_key, COUNT(*)
                   FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
                  WHERE pt.playlist_id = ?1 GROUP BY t.track_key",
            )
            .map_err(|err| db_error(cosa, &err))?;
        statement
            .query_map([playlist_id], |riga| {
                Ok((
                    riga.get::<_, String>(0)?,
                    usize::try_from(riga.get::<_, i64>(1)?).unwrap_or(0),
                ))
            })
            .map_err(|err| db_error(cosa, &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| db_error(cosa, &err))?
    };
    let mut prima_per_chiave: BTreeMap<&str, usize> = BTreeMap::new();
    for brano in prima {
        *prima_per_chiave.entry(brano.as_str()).or_insert(0) += 1;
    }

    // Le posizioni, nell'ordine vivo della sequenza, da seppellire: si fa dal
    // fondo, perché sotterrare un elemento sposta i numeri di quelli dopo.
    let ordine = sequenza.ordine();
    let mut posizioni: Vec<usize> = Vec::new();
    for (brano, quante_prima) in &prima_per_chiave {
        let da_togliere = quante_prima.saturating_sub(ora.get(*brano).copied().unwrap_or(0));
        let sue: Vec<usize> = ordine
            .iter()
            .enumerate()
            .filter(|(_, b)| *b == brano)
            .map(|(posizione, _)| posizione)
            .collect();
        posizioni.extend(sue.into_iter().rev().take(da_togliere));
    }
    posizioni.sort_unstable_by(|a, b| b.cmp(a));
    let mut sepolte = 0;
    for posizione in posizioni {
        if sequenza.togli(posizione, adesso_ms) {
            sepolte += 1;
        }
    }
    if sepolte > 0 {
        scrivi_sequenza(connection, &chiave, &sequenza)?;
    }
    Ok(sepolte)
}

// ── dal fuso al database ────────────────────────────────────────────────────

/// Cosa la sincronia ha cambiato, da raccontare a chi guarda.
///
/// Non è telemetria: è la sola cosa che permette di fidarsi di un automatismo che
/// scrive nella libreria da solo. «142 ascolti da *portatile*, 3 playlist
/// riordinate» è una frase che si può leggere; «sincronizzato» non è.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cambiamenti {
    /// Brani il cui conteggio d'ascolto è cambiato.
    pub ascolti: usize,
    /// Voti arrivati da fuori.
    pub voti: usize,
    /// Cuoricini messi o tolti da fuori.
    pub preferiti: usize,
    /// Posizioni di riascolto aggiornate.
    pub posizioni: usize,
    /// Playlist create o rifatte.
    pub playlist: usize,
    /// Playlist cancellate altrove.
    pub playlist_tolte: usize,
    /// Cartelle sorvegliate aggiunte o tolte.
    pub cartelle: usize,
    /// Cartelle accese altrove su un host di rete che questo PC non usa, e
    /// lasciate fuori: vedi [`applica_cartelle`].
    ///
    /// Non è una scrittura, e non conta per [`Cambiamenti::e_vuoto`]: serve a
    /// dirlo nel diario invece di tacerlo.
    #[serde(default)]
    pub cartelle_di_rete: usize,
    /// Dispositivi visti in questa passata.
    pub dispositivi: usize,
}

impl Cambiamenti {
    /// Nessuna scrittura: la libreria era già allineata.
    #[must_use]
    pub const fn e_vuoto(&self) -> bool {
        self.ascolti == 0
            && self.voti == 0
            && self.preferiti == 0
            && self.posizioni == 0
            && self.playlist == 0
            && self.playlist_tolte == 0
            && self.cartelle == 0
    }
}

/// Scrive nella libreria quel che la fusione ha deciso.
///
/// Una transazione sola: o la libreria è quella di prima, o è quella dopo. Una
/// sincronia interrotta a metà che lasciasse gli ascolti nuovi e le playlist
/// vecchie sarebbe indistinguibile, dall'esterno, da una che ha perso dei dati.
///
/// # Perché scrive da sé
///
/// È la decisione che questo ramo ha preso, e va detta dove si vede: la
/// sincronia applica senza chiedere. Regge perché un CRDT non cancella per
/// costruzione — ogni fusione qui dentro è commutativa, associativa e
/// idempotente, e i test di convergenza lo dimostrano invece di affermarlo.
/// L'unica cosa che sparisce è ciò che qualcuno ha cancellato *apposta*, e viaggia
/// come lapide.
///
/// # Errori
///
/// `db.queryFailed` se una scrittura fallisce.
pub fn applica(
    connection: &mut Connection,
    fuso: &Fuso,
    adesso_ms: i64,
) -> Result<Cambiamenti, AppError> {
    let tx = crate::library::transazione_di_scrittura(connection)
        .map_err(|err| db_error("apertura della sincronia", &err))?;

    let cambiamenti = applica_in(&tx, fuso, adesso_ms)?;

    tx.commit()
        .map_err(|err| db_error("chiusura della sincronia", &err))?;
    Ok(cambiamenti)
}

/// [`applica`], ma solo se playlist e cartelle sono ancora quelle di `partenza`.
///
/// Torna `None`, senza aver scritto niente, se sono cambiate.
///
/// # Perché
///
/// Fra la lettura del documento ([`contenuto`]) e l'applicazione la passata sta
/// in rete anche due minuti, senza lucchetto. Quasi tutto quel che [`applica`]
/// scrive si fonde con quel che trova — gli ascolti col massimo, voti, cuori e
/// posizioni con la loro data — e un cuore messo in quei due minuti sopravvive
/// da sé. Due cose no: l'appartenenza a una playlist si svuota e si riscrive
/// dalla sequenza fusa, e l'elenco delle cartelle si rifà dalla mappa. Un brano
/// aggiunto a una playlist, o una cartella aggiunta, in quei due minuti non
/// erano nel documento, quindi non erano nel fuso, e sparivano: in silenzio e
/// per sempre, perché la sequenza riscritta non li ricordava più.
///
/// Qui, nella stessa transazione che scriverà, si rifà l'allineamento e si
/// rilegge quella parte del documento. Se è cambiata non si scrive niente —
/// nemmeno l'allineamento, che rifarà la passata dopo con la sua ora. Non si
/// perde niente nemmeno così: i documenti degli altri restano nella memoria
/// della passata, e la passata dopo fonde di nuovo partendo dalla libreria
/// vera.
///
/// # Errori
///
/// `db.queryFailed` se una lettura o una scrittura fallisce.
pub fn applica_se_ferma(
    connection: &mut Connection,
    partenza: &Contenuto,
    fuso: &Fuso,
    adesso_ms: i64,
) -> Result<Option<Cambiamenti>, AppError> {
    let tx = crate::library::transazione_di_scrittura(connection)
        .map_err(|err| db_error("apertura della sincronia", &err))?;

    // Qui l'allineamento serve solo a vedere se qualcosa si è mosso, e il nome
    // del dispositivo non c'è (la firma è quella che la finestra chiama). Non
    // serve: un elemento che questo allineamento creasse cambierebbe la
    // sequenza, quindi `playlist` qui sotto non sarebbe più quella di
    // `partenza`, e la transazione cadrebbe senza `commit` — il nome con cui
    // l'avrebbe battezzato non arriva mai nel database. Gli elementi veri li
    // battezza l'`allinea` della passata dopo, col nome giusto.
    //
    // Per lo stesso motivo, una voce di un brano che qui manca non è un
    // cambiamento: `allinea_sequenze` la lascia viva e non tocca la sequenza,
    // quindi non ferma l'applicazione: una voce sepolta qui, come succedeva,
    // avrebbe cambiato `playlist` sotto e fatto cadere la passata di un brano
    // arrivato durante la rete. Quel brano si aggancia qui, nella stessa
    // transazione che scrive il fuso, e `applica_playlist` lo trova già al suo
    // posto quando rifà `playlist_tracks` dalla sequenza.
    allinea_in(&tx, ORIGINE, adesso_ms)?;
    let cartelle: BTreeMap<String, Interruttore> =
        settings::read_json(&tx, CHIAVE_CARTELLE_SINCRONIA)?.unwrap_or_default();
    if cartelle != partenza.cartelle || playlist(&tx)? != partenza.playlist {
        // `tx` cade senza `commit`: si torna indietro, allineamento compreso.
        return Ok(None);
    }

    let cambiamenti = applica_in(&tx, fuso, adesso_ms)?;
    tx.commit()
        .map_err(|err| db_error("chiusura della sincronia", &err))?;
    Ok(Some(cambiamenti))
}

/// Le stesse scritture di [`applica`], dentro una transazione già aperta.
///
/// # Perché esiste, e chi la usa
///
/// La sincronia non ne aveva bisogno: apre la sua transazione, scrive, chiude.
/// Il **profilo** sì. Un profilo importato mostra prima un piano — «cosa
/// cambierebbe» — e la regola di questo programma è che il piano non sia una
/// previsione scritta altrove ma *l'esecuzione annullata*: si esegue davvero,
/// dentro un punto di salvataggio, e lo si abbandona. Perché quella regola
/// resti vera anche per la parte di libreria del profilo, le scritture devono
/// poter girare dentro una transazione che decide **chi le chiama** se
/// confermare o buttare.
///
/// [`applica`] resta l'involucro di sempre, e chi sincronizza non cambia riga.
///
/// # Errori
///
/// `db.queryFailed` se una scrittura fallisce.
pub fn applica_in(
    tx: &Transaction<'_>,
    fuso: &Fuso,
    adesso_ms: i64,
) -> Result<Cambiamenti, AppError> {
    let mut cambiamenti = Cambiamenti {
        ascolti: applica_ascolti(tx, fuso)?,
        voti: applica_voti(tx, fuso)?,
        preferiti: applica_preferiti(tx, fuso)?,
        posizioni: applica_posizioni(tx, fuso)?,
        ..Cambiamenti::default()
    };
    applica_ultimi(tx, fuso)?;
    applica_lapidi(tx, fuso)?;
    cambiamenti.playlist_tolte = togli_playlist(tx, fuso)?;
    cambiamenti.playlist = applica_playlist(tx, fuso)?;
    // Le sequenze sono appena cambiate (o sono state tolte), e con loro i brani
    // che nominano senza che qui ci siano: vedi [`CHIAVE_BRANI_ASSENTI`]. Nella
    // stessa transazione, perché il piano di un profilo che si abbandona non
    // lasci una memoria che non corrisponde ai dati.
    ricorda_i_brani_assenti(tx)?;
    (cambiamenti.cartelle, cambiamenti.cartelle_di_rete) = applica_cartelle(tx, fuso)?;
    applica_skin(tx, fuso)?;
    cambiamenti.dispositivi = applica_dispositivi(tx, fuso, adesso_ms)?;
    Ok(cambiamenti)
}

/// Gli ascolti di tutti i dispositivi, e la somma in `tracks`.
fn applica_ascolti(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    if fuso.ascolti.is_empty() {
        return Ok(0);
    }
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_ascolti (track_key, dispositivo, quanti) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_key, dispositivo) DO UPDATE SET
               -- Il massimo, mai il valore che arriva: il numero di un dispositivo
               -- sale solo per mano sua, e un documento vecchio che passa di qui
               -- non deve poter far scendere quello che avevamo già letto.
               quanti = max(sync_ascolti.quanti, excluded.quanti)
             WHERE excluded.quanti > sync_ascolti.quanti",
        )
        .map_err(|err| db_error("ascolti ricevuti", &err))?;

    // Gli ascolti di un brano che qui non c'è si **tengono** — `sync_ascolti`
    // è per chiave, e `tracks.play_count` lo raggiunge al primo `allinea` dopo
    // che la scansione ha portato il brano: la scansione da sola non lo fa — ma
    // non sono «brani il cui conteggio è cambiato»: nessun brano è cambiato.
    // Contarli faceva dire al piano di un profilo «3 brani con conteggi nuovi»
    // su una libreria vuota (T3-3).
    let mut ha_il_brano = tx
        .prepare("SELECT EXISTS(SELECT 1 FROM tracks WHERE track_key = ?1)")
        .map_err(|err| db_error("ascolti ricevuti", &err))?;
    let mut toccati = 0;
    let mut scritti = false;
    for (brano, contatore) in &fuso.ascolti {
        let mut cambiato = false;
        for (dispositivo, quanti) in contatore.dispositivi() {
            if quanti <= 0 {
                continue;
            }
            let scritte = scrivi
                .execute(rusqlite::params![brano, dispositivo, quanti])
                .map_err(|err| db_error("ascolti ricevuti", &err))?;
            cambiato |= scritte > 0;
        }
        if !cambiato {
            continue;
        }
        scritti = true;
        let c_e: bool = ha_il_brano
            .query_row([brano], |row| row.get(0))
            .map_err(|err| db_error("ascolti ricevuti", &err))?;
        if c_e {
            toccati += 1;
        }
    }

    if scritti {
        tx.execute(
            "UPDATE tracks
                SET play_count = COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                            WHERE a.track_key = tracks.track_key), 0)
              WHERE play_count <> COALESCE((SELECT SUM(a.quanti) FROM sync_ascolti a
                                             WHERE a.track_key = tracks.track_key), 0)
                AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
            [],
        )
        .map_err(|err| db_error("somma degli ascolti", &err))?;
    }
    Ok(toccati)
}

/// L'ultimo ascolto: il più recente fra i due.
fn applica_ultimi(tx: &Transaction<'_>, fuso: &Fuso) -> Result<(), AppError> {
    let mut scrivi = tx
        .prepare(
            "UPDATE tracks SET last_played_at = ?2
              WHERE track_key = ?1 AND COALESCE(last_played_at, 0) < ?2 AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
        )
        .map_err(|err| db_error("ultimi ascolti ricevuti", &err))?;
    for (brano, quando) in &fuso.ultimo {
        scrivi
            .execute(rusqlite::params![brano, quando])
            .map_err(|err| db_error("ultimi ascolti ricevuti", &err))?;
    }
    Ok(())
}

/// I voti, con la loro data.
fn applica_voti(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "UPDATE tracks
                SET rating = ?2, rating_at = ?3, stats_updated_at = max(stats_updated_at, ?3)
              WHERE track_key = ?1 AND COALESCE(rating_at, -1) < ?3 AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
        )
        .map_err(|err| db_error("voti ricevuti", &err))?;
    let mut cambiati = 0;
    for (brano, voto) in &fuso.voti {
        let scritte = scrivi
            .execute(rusqlite::params![brano, i64::from(voto.v), voto.at])
            .map_err(|err| db_error("voti ricevuti", &err))?;
        if scritte > 0 {
            cambiati += 1;
        }
    }
    Ok(cambiati)
}

/// I preferiti, messi e tolti.
fn applica_preferiti(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "UPDATE tracks
                SET liked = ?2, liked_at = ?3, stats_updated_at = max(stats_updated_at, ?3)
              WHERE track_key = ?1 AND COALESCE(liked_at, -1) < ?3 AND NOT (path >= 'lontano://' AND path < 'lontano:/0')",
        )
        .map_err(|err| db_error("preferiti ricevuti", &err))?;
    let mut cambiati = 0;
    for (brano, stato) in &fuso.preferiti {
        let scritte = scrivi
            .execute(rusqlite::params![brano, i64::from(stato.on), stato.at])
            .map_err(|err| db_error("preferiti ricevuti", &err))?;
        if scritte > 0 {
            cambiati += 1;
        }
    }
    Ok(cambiati)
}

/// Dove si era arrivati.
fn applica_posizioni(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_posizioni (track_key, ms, at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_key) DO UPDATE SET ms = excluded.ms, at_ms = excluded.at_ms
             WHERE excluded.at_ms > sync_posizioni.at_ms",
        )
        .map_err(|err| db_error("posizioni ricevute", &err))?;
    let mut cambiate = 0;
    for (brano, momento) in &fuso.posizioni {
        let scritte = scrivi
            .execute(rusqlite::params![brano, momento.ms.max(0), momento.at])
            .map_err(|err| db_error("posizioni ricevute", &err))?;
        if scritte > 0 {
            cambiate += 1;
        }
    }
    Ok(cambiate)
}

/// Le lapidi: entrano tutte, e restano.
///
/// Anche quelle di brani che qui non ci sono mai stati. Una lapide che smette di
/// viaggiare è un brano cancellato che torna: basta un dispositivo rimasto
/// indietro a rimandarlo indietro alla prima passata.
fn applica_lapidi(tx: &Transaction<'_>, fuso: &Fuso) -> Result<(), AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(kind, key) DO UPDATE SET
               deleted_at = min(sync_tombstones.deleted_at, excluded.deleted_at)",
        )
        .map_err(|err| db_error("lapidi ricevute", &err))?;
    for (chiave, quando) in &fuso.lapidi.brani {
        scrivi
            .execute(rusqlite::params!["track", chiave, quando])
            .map_err(|err| db_error("lapidi ricevute", &err))?;
    }
    for (chiave, quando) in &fuso.lapidi.playlist {
        scrivi
            .execute(rusqlite::params!["playlist", chiave, quando])
            .map_err(|err| db_error("lapidi ricevute", &err))?;
    }
    Ok(())
}

/// Toglie le playlist cancellate altrove.
///
/// Solo le playlist, mai i brani — ed è la stessa asimmetria di `backup.rs`, non
/// una distrazione. Una riga di brano non la crea mai una sincronia: la crea la
/// scansione, leggendo un file che c'è. Cancellarla qui vorrebbe dire buttare via
/// una storia d'ascolto senza togliere niente in cambio, e alla scansione
/// successiva il brano tornerebbe comunque, senza la sua storia.
fn togli_playlist(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    if fuso.lapidi.playlist.is_empty() {
        return Ok(0);
    }
    let mut cancella = tx
        .prepare(
            "DELETE FROM playlists WHERE playlist_key = ?1 AND updated_at <= ?2 AND is_smart = 0",
        )
        .map_err(|err| db_error("playlist cancellate", &err))?;
    let mut sgombera = tx
        .prepare("DELETE FROM sync_sequenza WHERE playlist_key = ?1")
        .map_err(|err| db_error("playlist cancellate", &err))?;
    let mut tolte = 0;
    for (chiave, quando) in &fuso.lapidi.playlist {
        // Una playlist rifatta dopo essere stata cancellata sopravvive: la
        // fusione l'ha già tenuta, e qui la si riconosce dal fatto che è più
        // recente della lapide.
        if fuso.playlist.contains_key(chiave) {
            continue;
        }
        let via = cancella
            .execute(rusqlite::params![chiave, quando])
            .map_err(|err| db_error("playlist cancellate", &err))?;
        if via > 0 {
            sgombera
                .execute([chiave])
                .map_err(|err| db_error("playlist cancellate", &err))?;
            tolte += 1;
        }
    }
    Ok(tolte)
}

/// Le playlist: la riga, la sequenza, e l'appartenenza compattata.
fn applica_playlist(tx: &Transaction<'_>, fuso: &Fuso) -> Result<usize, AppError> {
    if fuso.playlist.is_empty() {
        return Ok(0);
    }
    let mut inserisci = tx
        .prepare(
            "INSERT INTO playlists
                 (playlist_key, name, description, created_at, updated_at, is_smart, rules)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(playlist_key) DO UPDATE SET
                 name        = excluded.name,
                 description = excluded.description,
                 -- La data di creazione più antica delle due: è quella vera, e una
                 -- sincronia non deve far sembrare appena nata una playlist che
                 -- esiste da tre anni.
                 created_at  = min(playlists.created_at, excluded.created_at),
                 updated_at  = max(playlists.updated_at, excluded.updated_at),
                 is_smart    = excluded.is_smart,
                 rules       = excluded.rules",
        )
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut identifica = tx
        .prepare("SELECT id FROM playlists WHERE playlist_key = ?1")
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut cerca_brano = tx
        .prepare("SELECT id FROM tracks WHERE track_key = ?1 ORDER BY (path >= 'lontano://' AND path < 'lontano:/0'), id LIMIT 1")
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut svuota = tx
        .prepare("DELETE FROM playlist_tracks WHERE playlist_id = ?1")
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut accoda = tx
        .prepare(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        )
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut dissotterra = tx
        .prepare("DELETE FROM sync_tombstones WHERE kind = 'playlist' AND key = ?1")
        .map_err(|err| db_error("playlist ricevute", &err))?;
    let mut membri_attuali = tx
        .prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position")
        .map_err(|err| db_error("playlist ricevute", &err))?;

    // Com'è adesso, per non riscrivere quel che è già così. Il fuso contiene
    // anche le playlist del proprio documento: senza il confronto ogni passata
    // rifaceva l'upsert di tutte, riscriveva tutte le sequenze e svuotava e
    // riempiva tutto `playlist_tracks` — col lucchetto della libreria preso,
    // lo stesso del lettore — e le contava fra le «arrivate», così il
    // resoconto non diceva mai «era già tutto allineato».
    let locali = playlist(tx)?;

    let mut scritte = 0;
    for (chiave, playlist) in &fuso.playlist {
        let locale = locali.get(chiave);
        // La riga resterebbe com'è: stessi campi, e la data di creazione e
        // quella di modifica che l'upsert terrebbe (la minima e la massima).
        let riga_uguale = locale.is_some_and(|qui| {
            qui.nome == playlist.nome
                && qui.descrizione == playlist.descrizione
                && qui.creata_at <= playlist.creata_at
                && qui.at >= playlist.at
                && qui.smart == playlist.smart
                && qui.regole == playlist.regole
        });
        if !riga_uguale {
            inserisci
                .execute(rusqlite::params![
                    chiave,
                    playlist.nome,
                    playlist.descrizione,
                    playlist.creata_at,
                    playlist.at,
                    i64::from(playlist.smart),
                    playlist.regole,
                ])
                .map_err(|err| db_error("playlist ricevute", &err))?;
        }
        // Sempre, anche quando la riga c'è già uguale: una playlist ricreata
        // a mano qui (`playlists::crea_riga`) non toglie la propria lapide, e
        // questa è la passata che lo fa.
        dissotterra
            .execute([chiave])
            .map_err(|err| db_error("playlist ricevute", &err))?;

        if playlist.smart {
            if !riga_uguale {
                scritte += 1;
            }
            continue;
        }
        // Una playlist che arriva e qui non c'è — nuova, o rifatta altrove
        // dopo una cancellazione, o tenuta in vita da chi l'ha ritoccata dopo
        // la lapide — non eredita le righe che stavano sotto la sua chiave.
        // `scrivi_sequenza` le fonderebbe con `min(tolto)`, e le lapidi della
        // playlist di prima (una cancellazione fatta qui seppellisce tutto, e
        // i battesimi `origine:*` danno le stesse identità a playlist diverse
        // con lo stesso nome) ucciderebbero gli elementi vivi di quella che
        // arriva: proprio quelli dei brani che qui non ci sono, che nessun
        // `allinea` di qui può ricreare. La playlist che il fuso tiene ha già
        // vinto sulla lapide di playlist: torna intera (revisione di FSYNC2,
        // R1 e R2).
        if locale.is_none() {
            tx.execute(
                "DELETE FROM sync_sequenza WHERE playlist_key = ?1",
                [chiave],
            )
            .map_err(|err| db_error("sequenza di una playlist di prima", &err))?;
        }
        // La sequenza intera, lapidi comprese, e non solo l'ordine che ne
        // esce: elementi tolti ed elementi di brani che qui non ci sono vanno
        // salvati lo stesso (vedi `Sequenza::elementi`).
        if locale.is_none_or(|qui| qui.sequenza != playlist.sequenza) {
            scrivi_sequenza(tx, chiave, &playlist.sequenza)?;
        }

        let id: i64 = identifica
            .query_row([chiave], |row| row.get(0))
            .map_err(|err| db_error("playlist ricevute", &err))?;

        // I brani che qui non ci sono si saltano nell'appartenenza ma **restano**
        // nella sequenza: il giorno che il file arriva, il brano ricompare al suo
        // posto invece che in fondo. È la differenza fra una playlist che
        // sopravvive a una libreria incompleta e una che si accorcia ogni volta.
        let mut brani = Vec::new();
        for chiave_brano in playlist.sequenza.ordine() {
            let trovato: Option<i64> = cerca_brano
                .query_row([chiave_brano], |row| row.get(0))
                .or_else(|err| match err {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    altro => Err(altro),
                })
                .map_err(|err| db_error("playlist ricevute", &err))?;
            if let Some(track_id) = trovato {
                brani.push(track_id);
            }
        }

        // Uguali a quel che c'è: niente da riscrivere, e niente da contare se
        // anche la riga è rimasta com'era.
        let attuali: Vec<i64> = membri_attuali
            .query_map([id], |row| row.get(0))
            .map_err(|err| db_error("playlist ricevute", &err))?
            .collect::<Result<_, _>>()
            .map_err(|err| db_error("playlist ricevute", &err))?;
        if attuali == brani {
            if !riga_uguale {
                scritte += 1;
            }
            continue;
        }
        scritte += 1;

        // Si svuota e si riscrive: `playlist_tracks` ha `PRIMARY KEY (playlist_id,
        // position)` e il vincolo si verifica a ogni istruzione, non alla COMMIT.
        // Le posizioni escono `0..n` senza buchi, che è la forma che la finestra
        // usa come indici.
        svuota
            .execute([id])
            .map_err(|err| db_error("playlist ricevute", &err))?;
        for (posizione, track_id) in brani.iter().enumerate() {
            let posizione = i64::try_from(posizione).unwrap_or(i64::MAX);
            accoda
                .execute(rusqlite::params![id, track_id, posizione])
                .map_err(|err| db_error("playlist ricevute", &err))?;
        }
    }
    Ok(scritte)
}

/// Le cartelle sorvegliate: la mappa datata, e l'elenco che ne discende.
///
/// L'elenco si rifà dalla mappa invece di essere unito: è l'unico modo perché una
/// cartella **tolta** resti tolta. Le cartelle sono percorsi di un computer
/// preciso, e quelle di un altro qui non esistono — ma entrano ugualmente
/// nell'elenco, come già fa un ripristino da backup: una cartella che non c'è
/// costa una scansione a vuoto, mentre una cartella che manca costa una libreria
/// che non si popola e nessuno sa perché.
///
/// Lo stesso, e a maggior ragione, per le cartelle del cloud (`nuvola://…`):
/// viaggiano come sono, perché il conto nel percorso è lo stesso su ogni
/// dispositivo che collega quell'account. Di là compaiono «da collegare»; lo
/// specchio del servizio non viaggia — si rielenca.
fn applica_cartelle(tx: &Transaction<'_>, fuso: &Fuso) -> Result<(usize, usize), AppError> {
    if fuso.cartelle.is_empty() {
        return Ok((0, 0));
    }
    // Gli host di rete che questo PC usa già, letti **prima** di scrivere:
    // una cartella del documento non deve poter rendere «noto» il proprio host.
    let host_noti = crate::profilo::host_di_questo_pc(tx)?;
    let mut mappa: BTreeMap<String, Interruttore> =
        settings::read_json(tx, CHIAVE_CARTELLE_SINCRONIA)?.unwrap_or_default();
    let mut cambiate = 0;
    let mut di_rete = 0;
    for (percorso, stato) in &fuso.cartelle {
        // Una cartella accesa su un host di rete che questo PC non usa resta
        // fuori, e fuori anche dalla mappa: in `library.roots` la scansione e
        // le Impostazioni la aprirebbero, cioè una connessione SMB verso un
        // host scelto da chi ha scritto il documento, senza che nessuno qui
        // l'abbia chiesta. È la stessa regola del piano del profilo
        // (`profilo::host_di_questo_pc`); `MAX_SONDE` qui non serve, perché
        // la sincronia non sonda niente. Nella mappa non si scrive nemmeno
        // spenta: il documento di qui la ripubblicherebbe, e spenta la
        // toglierebbe agli altri dispositivi che la usano. Chi la vuole qui
        // la aggiunge a mano, e da lì il suo host è noto.
        if stato.on && crate::profilo::verso_un_host_ignoto(percorso, &host_noti) {
            di_rete += 1;
            continue;
        }
        match mappa.get(percorso) {
            Some(gia) if gia.at >= stato.at => {}
            Some(gia) => {
                if gia.on != stato.on {
                    cambiate += 1;
                }
                mappa.insert(percorso.clone(), *stato);
            }
            None => {
                mappa.insert(percorso.clone(), *stato);
                cambiate += 1;
            }
        }
    }
    settings::write_json(tx, CHIAVE_CARTELLE_SINCRONIA, &mappa)?;

    let elenco: Vec<&str> = mappa
        .iter()
        .filter(|(_, stato)| stato.on)
        .map(|(percorso, _)| percorso.as_str())
        .collect();
    settings::write_json(tx, CHIAVE_CARTELLE, &elenco)?;
    Ok((cambiate, di_rete))
}

/// La skin scelta per ultima.
fn applica_skin(tx: &Transaction<'_>, fuso: &Fuso) -> Result<(), AppError> {
    let Some(scelta) = &fuso.skin_attiva else {
        return Ok(());
    };
    let mia: i64 = settings::read(tx, CHIAVE_SKIN_QUANDO)?
        .and_then(|quando| quando.parse().ok())
        .unwrap_or(0);
    if scelta.at <= mia {
        return Ok(());
    }
    // La skin si scrive anche se il pacchetto qui non c'è: chi la legge ricade
    // sulla skin di serie quando non la trova, e il giorno che il `.ryskin`
    // arriva dalla nuvola la scelta è già quella giusta. Il contrario — rifiutare
    // la scelta finché il file non c'è — produrrebbe due dispositivi con due skin
    // diverse e nessun modo di accorgersi del perché.
    settings::write(tx, CHIAVE_SKIN, &scelta.id)?;
    settings::write(tx, CHIAVE_SKIN_QUANDO, &scelta.at.to_string())?;
    Ok(())
}

/// I dispositivi visti in questa passata.
fn applica_dispositivi(
    tx: &Transaction<'_>,
    fuso: &Fuso,
    adesso_ms: i64,
) -> Result<usize, AppError> {
    let mut scrivi = tx
        .prepare(
            "INSERT INTO sync_dispositivi (id, visto_ms) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET visto_ms = excluded.visto_ms",
        )
        .map_err(|err| db_error("dispositivi visti", &err))?;
    for dispositivo in &fuso.dispositivi {
        scrivi
            .execute(rusqlite::params![dispositivo, adesso_ms])
            .map_err(|err| db_error("dispositivi visti", &err))?;
    }
    Ok(fuso.dispositivi.len())
}

// ── i dispositivi, per chi li deve mostrare ─────────────────────────────────

/// Un dispositivo conosciuto.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Dispositivo {
    /// L'identificativo con cui firma i suoi documenti.
    pub id: String,
    /// Come si chiama, se qualcuno gliel'ha detto.
    pub nome: Option<String>,
    /// La sua chiave pubblica, base64url.
    pub chiave: Option<String>,
    /// Se ci si fida di quel che scrive.
    pub fidato: bool,
    /// L'ultima volta che il suo documento è stato letto.
    pub visto_ms: Option<i64>,
}

/// I dispositivi conosciuti, i più visti di recente per primi.
///
/// # Errori
///
/// `db.queryFailed` se la lettura fallisce.
pub fn dispositivi(connection: &Connection) -> Result<Vec<Dispositivo>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, nome, chiave, fidato, visto_ms FROM sync_dispositivi
              ORDER BY visto_ms DESC NULLS LAST, id",
        )
        .map_err(|err| db_error("dispositivi", &err))?;
    let righe = statement
        .query_map([], |row| {
            let fidato: i64 = row.get(3)?;
            Ok(Dispositivo {
                id: row.get(0)?,
                nome: row.get(1)?,
                chiave: row.get(2)?,
                fidato: fidato != 0,
                visto_ms: row.get(4)?,
            })
        })
        .map_err(|err| db_error("dispositivi", &err))?;
    let mut elenco = Vec::new();
    for riga in righe {
        elenco.push(riga.map_err(|err| db_error("dispositivi", &err))?);
    }
    Ok(elenco)
}

/// Registra i dispositivi visti ma non ancora accettati.
///
/// Senza `fidato`, e senza toccarlo se la riga c'è già: un dispositivo accettato
/// non deve tornare ignoto perché lo si è rivisto. `visto_ms` invece si aggiorna
/// sempre — è l'unica cosa che distingue un dispositivo ancora in uso da uno che
/// non scrive più da mesi.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn intravisti(
    connection: &Connection,
    elenco: &[String],
    adesso_ms: i64,
) -> Result<usize, AppError> {
    if elenco.is_empty() {
        return Ok(0);
    }
    let mut scrivi = connection
        .prepare(
            "INSERT INTO sync_dispositivi (id, visto_ms) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET visto_ms = excluded.visto_ms",
        )
        .map_err(|err| db_error("dispositivi intravisti", &err))?;
    for id in elenco {
        scrivi
            .execute(rusqlite::params![id, adesso_ms])
            .map_err(|err| db_error("dispositivi intravisti", &err))?;
    }
    Ok(elenco.len())
}

/// Accetta un dispositivo: da qui in poi quel che scrive vale.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn fidati(
    connection: &Connection,
    id: &str,
    nome: Option<&str>,
    chiave: Option<&str>,
) -> Result<(), AppError> {
    connection
        .execute(
            "INSERT INTO sync_dispositivi (id, nome, chiave, fidato) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(id) DO UPDATE SET
               nome   = COALESCE(excluded.nome, sync_dispositivi.nome),
               chiave = COALESCE(excluded.chiave, sync_dispositivi.chiave),
               fidato = 1",
            rusqlite::params![id, nome, chiave],
        )
        .map_err(|err| db_error("fiducia in un dispositivo", &err))?;
    Ok(())
}

/// Dà un nome a un dispositivo, e non tocca nient'altro.
///
/// Non è [`fidati`]: dare un nome riconoscibile a un dispositivo «da
/// accettare» prima di decidere non è decidere, e passare da lì lo rendeva
/// fidato — da quel momento i suoi documenti valevano (R13-3). Un nome vuoto
/// toglie il nome, e si torna all'identificativo. Un dispositivo che non c'è
/// non nasce: restituisce `false`.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn rinomina(connection: &Connection, id: &str, nome: Option<&str>) -> Result<bool, AppError> {
    let nome = nome.map(str::trim).filter(|nome| !nome.is_empty());
    let toccate = connection
        .execute(
            "UPDATE sync_dispositivi SET nome = ?2 WHERE id = ?1",
            rusqlite::params![id, nome],
        )
        .map_err(|err| db_error("nome di un dispositivo", &err))?;
    Ok(toccate > 0)
}

/// Dimentica un dispositivo.
///
/// Non tocca gli ascolti che ha già portato: erano ascolti veri, e cancellarli
/// perché non si vuole più sentire quel telefono significherebbe che dimenticare
/// un dispositivo riscrive la storia della libreria. Il suo documento smette
/// semplicemente di essere letto.
///
/// # Errori
///
/// `db.queryFailed` se la scrittura fallisce.
pub fn dimentica(connection: &Connection, id: &str) -> Result<bool, AppError> {
    let via = connection
        .execute("DELETE FROM sync_dispositivi WHERE id = ?1", [id])
        .map_err(|err| db_error("dispositivo dimenticato", &err))?;
    Ok(via > 0)
}

#[cfg(test)]
mod prove {
    use rymembra_sync::{Documento, fondi};

    use super::*;
    use crate::db;

    /// Una libreria con qualche brano e una playlist.
    fn libreria() -> Connection {
        let connection = db::open_in_memory()
            .expect("database in memoria")
            .connection;
        for (id, chiave) in [(1, "a"), (2, "b"), (3, "c")] {
            connection
                .execute(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                         date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?3, 'chi', 'dove', 0, 0, 0)",
                    rusqlite::params![id, format!("/musica/{chiave}.flac"), chiave],
                )
                .expect("brano");
        }
        connection
    }

    fn playlist_con(connection: &Connection, chiave: &str, brani: &[i64]) {
        connection
            .execute(
                "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                 VALUES (?1, ?1, 100, 100)",
                [chiave],
            )
            .expect("playlist");
        let id: i64 = connection
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = ?1",
                [chiave],
                |row| row.get(0),
            )
            .expect("identificativo");
        for (posizione, brano) in brani.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![id, brano, i64::try_from(posizione).expect("posizione")],
                )
                .expect("membro");
        }
    }

    fn ordine(connection: &Connection, chiave: &str) -> Vec<String> {
        let mut statement = connection
            .prepare(
                "SELECT t.track_key FROM playlist_tracks pt
                   JOIN playlists p ON p.id = pt.playlist_id
                   JOIN tracks t    ON t.id = pt.track_id
                  WHERE p.playlist_key = ?1 ORDER BY pt.position",
            )
            .expect("ordine");
        let righe = statement
            .query_map([chiave], |row| row.get::<_, String>(0))
            .expect("ordine");
        righe.map(|riga| riga.expect("riga")).collect()
    }

    fn conteggio(connection: &Connection, chiave: &str) -> i64 {
        connection
            .query_row(
                "SELECT play_count FROM tracks WHERE track_key = ?1",
                [chiave],
                |row| row.get(0),
            )
            .expect("conteggio")
    }

    #[test]
    fn lo_storico_che_c_era_diventa_dello_pseudo_dispositivo_importazione() {
        let mut connection = libreria();
        connection
            .execute("UPDATE tracks SET play_count = 7 WHERE track_key = 'a'", [])
            .expect("storico");

        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let quanti: i64 = connection
            .query_row(
                "SELECT quanti FROM sync_ascolti WHERE track_key = 'a' AND dispositivo = ?1",
                [IMPORTAZIONE],
                |row| row.get(0),
            )
            .expect("importazione");
        assert_eq!(quanti, 7);
        assert_eq!(
            conteggio(&connection, "a"),
            7,
            "la somma non cambia il totale"
        );
    }

    #[test]
    fn allineare_due_volte_non_raddoppia_niente() {
        let mut connection = libreria();
        connection
            .execute("UPDATE tracks SET play_count = 4 WHERE track_key = 'b'", [])
            .expect("storico");

        for _ in 0..5 {
            allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        }
        assert_eq!(conteggio(&connection, "b"), 4);
    }

    #[test]
    fn un_ascolto_si_somma_allo_storico_invece_di_sostituirlo() {
        let mut connection = libreria();
        connection
            .execute("UPDATE tracks SET play_count = 4 WHERE track_key = 'b'", [])
            .expect("storico");
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let tx = connection.transaction().expect("transazione");
        conta_ascolto(&tx, 2, "portatile").expect("ascolto");
        tx.commit().expect("commit");

        assert_eq!(conteggio(&connection, "b"), 5);
    }

    #[test]
    fn un_conteggio_ripristinato_senza_allineamento_sopravvive_al_primo_ascolto() {
        // Un ripristino da backup o un'importazione scrivono `play_count` e non
        // toccano `sync_ascolti`; con la sincronia spenta, che è il default,
        // nessun `allinea` passa prima del primo ascolto.
        let mut connection = libreria();
        connection
            .execute(
                "UPDATE tracks SET play_count = 40 WHERE track_key = 'a'",
                [],
            )
            .expect("conteggio ripristinato");

        let tx = connection.transaction().expect("transazione");
        conta_ascolto(&tx, 1, "portatile").expect("ascolto");
        tx.commit().expect("commit");

        assert_eq!(conteggio(&connection, "a"), 41, "quaranta più uno, non uno");
        let ereditati: i64 = connection
            .query_row(
                "SELECT quanti FROM sync_ascolti WHERE track_key = 'a' AND dispositivo = ?1",
                [IMPORTAZIONE],
                |row| row.get(0),
            )
            .expect("importazione");
        assert_eq!(ereditati, 40);

        // E l'allineamento di dopo non trova niente da aggiungere.
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        assert_eq!(conteggio(&connection, "a"), 41);
    }

    #[test]
    fn gli_ascolti_di_due_dispositivi_si_sommano_nella_libreria() {
        let mut connection = libreria();
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let tx = connection.transaction().expect("transazione");
        for _ in 0..3 {
            conta_ascolto(&tx, 1, "portatile").expect("ascolto");
        }
        tx.commit().expect("commit");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = Contenuto::default();
        suo.ascolti.insert("a".to_owned(), 2);

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(
            conteggio(&connection, "a"),
            5,
            "tre più due, non tre né due"
        );
    }

    #[test]
    fn una_passata_a_vuoto_non_cambia_la_libreria() {
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1, 2]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[Documento::nuovo("portatile", mio.clone(), 2_000)]);
        // Ogni riga di `playlist_tracks` tolta si conta: una passata a vuoto non
        // deve riscrivere niente, né contare la playlist fra le «arrivate».
        connection
            .execute_batch(
                "CREATE TEMP TABLE tolte (n INTEGER);
                 CREATE TEMP TRIGGER conta_tolte AFTER DELETE ON playlist_tracks
                 BEGIN INSERT INTO tolte VALUES (1); END;",
            )
            .expect("spia");
        let cambiamenti = applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(cambiamenti.playlist_tolte, 0);
        assert_eq!(
            cambiamenti.playlist, 0,
            "niente è arrivato: {cambiamenti:?}"
        );
        assert!(cambiamenti.e_vuoto(), "{cambiamenti:?}");
        let tolte: i64 = connection
            .query_row("SELECT COUNT(*) FROM tolte", [], |row| row.get(0))
            .expect("spia");
        assert_eq!(tolte, 0, "playlist_tracks non si riscrive");
        assert_eq!(ordine(&connection, "sera"), vec!["a", "b"]);
        let dopo = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        assert_eq!(mio, dopo, "il documento è lo stesso di prima");

        // E quando qualcosa arriva davvero, si conta.
        let mut suo = mio.clone();
        if let Some(playlist) = suo.playlist.get_mut("sera") {
            playlist.sequenza.accoda("telefono", "c");
            playlist.at = 2_500;
        }
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 3_000),
            Documento::nuovo("telefono", suo, 3_000),
        ]);
        let cambiamenti = applica(&mut connection, &fuso, 3_000).expect("applicazione");
        assert_eq!(cambiamenti.playlist, 1);
        assert_eq!(ordine(&connection, "sera"), vec!["a", "b", "c"]);
    }

    #[test]
    fn due_dispositivi_che_aggiungono_un_brano_ciascuno_li_tengono_entrambi() {
        // Il caso che oggi si perde: due aggiunte in contemporanea alla stessa
        // playlist, e l'ultimo che scrive cancella l'altro.
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let comune = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");

        let mut suo = comune.clone();
        if let Some(playlist) = suo.playlist.get_mut("sera") {
            playlist.sequenza.accoda("telefono", "c");
            playlist.at = 1_500;
        }

        // Intanto, di qua, si aggiunge un altro brano per la via normale.
        let id: i64 = connection
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = 'sera'",
                [],
                |row| row.get(0),
            )
            .expect("playlist");
        connection
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, 2, 1)",
                [id],
            )
            .expect("aggiunta locale");
        connection
            .execute("UPDATE playlists SET updated_at = 1600 WHERE id = ?1", [id])
            .expect("data");
        allinea(&mut connection, "portatile", 1_600).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        let finale = ordine(&connection, "sera");
        assert!(
            finale.contains(&"b".to_owned()),
            "l'aggiunta locale resta: {finale:?}"
        );
        assert!(
            finale.contains(&"c".to_owned()),
            "quella remota arriva: {finale:?}"
        );
        assert_eq!(finale.len(), 3);
    }

    #[test]
    fn due_librerie_che_aggiungono_ciascuna_un_brano_li_tengono_entrambe() {
        // Come il caso qui sopra, ma con due dispositivi veri: tutti e due
        // passano da `allinea`, e nessuno dei due finge l'altro con `accoda`.
        // Con lo stesso nome per tutti, le due aggiunte nascevano `origine:1`
        // da tutte e due le parti, e la fusione ne teneva una.
        let mut portatile = libreria();
        let mut fisso = libreria();
        playlist_con(&portatile, "sera", &[1]);
        playlist_con(&fisso, "sera", &[1]);
        allinea(&mut portatile, "portatile", 1_000).expect("allineamento");
        allinea(&mut fisso, "fisso", 1_000).expect("allineamento");

        let passata = |portatile: &mut Connection, fisso: &mut Connection, adesso: i64| {
            let mio = contenuto(portatile, "portatile", BTreeMap::new(), BTreeMap::new())
                .expect("contenuto");
            let suo =
                contenuto(fisso, "fisso", BTreeMap::new(), BTreeMap::new()).expect("contenuto");
            let fuso = fondi(&[
                Documento::nuovo("portatile", mio.clone(), adesso),
                Documento::nuovo("fisso", suo.clone(), adesso),
            ]);
            applica_se_ferma(portatile, &mio, &fuso, adesso)
                .expect("applicazione")
                .expect("ferma");
            applica_se_ferma(fisso, &suo, &fuso, adesso)
                .expect("applicazione")
                .expect("ferma");
        };
        passata(&mut portatile, &mut fisso, 2_000);

        let aggiungi = |connection: &mut Connection, brano: i64, dispositivo: &str| {
            connection
                .execute(
                    "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                     SELECT id, ?1, 1 FROM playlists WHERE playlist_key = 'sera'",
                    [brano],
                )
                .expect("aggiunta");
            allinea(connection, dispositivo, 3_000).expect("allineamento");
        };
        aggiungi(&mut portatile, 2, "portatile");
        aggiungi(&mut fisso, 3, "fisso");
        passata(&mut portatile, &mut fisso, 4_000);

        for (chi, connection) in [("portatile", &portatile), ("fisso", &fisso)] {
            let finale = ordine(connection, "sera");
            assert!(finale.contains(&"b".to_owned()), "{chi}: {finale:?}");
            assert!(finale.contains(&"c".to_owned()), "{chi}: {finale:?}");
            assert_eq!(finale.len(), 3, "{chi}: {finale:?}");
        }
        assert_eq!(ordine(&portatile, "sera"), ordine(&fisso, "sera"));
    }

    #[test]
    fn due_playlist_omonime_mai_sincronizzate_collidono_per_posizione() {
        // Il limite dichiarato in [`ORIGINE`], fissato qui perché il commento
        // non possa tornare a dire «si aggiunge il resto» senza che una prova
        // se ne accorga: `origine:k` di qua e di là sono lo stesso elemento, e
        // per ogni posizione la fusione tiene la coppia minore.
        let mut qui = libreria();
        let mut la = libreria();
        playlist_con(&qui, "corsa", &[1, 2]);
        playlist_con(&la, "corsa", &[3, 1]);
        allinea(&mut qui, "portatile", 1_000).expect("allineamento");
        allinea(&mut la, "telefono", 1_000).expect("allineamento");

        let mio =
            contenuto(&qui, "portatile", BTreeMap::new(), BTreeMap::new()).expect("contenuto");
        let suo = contenuto(&la, "telefono", BTreeMap::new(), BTreeMap::new()).expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        let ordine_fuso = fuso.playlist["corsa"].sequenza.ordine();
        assert_eq!(
            ordine_fuso,
            vec!["a", "a"],
            "due posizioni, due brani: «b» e «c» si perdono"
        );
    }

    #[test]
    fn una_playlist_cancellata_altrove_se_ne_va() {
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1, 2]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = Contenuto::default();
        suo.lapidi.playlist.insert("sera".to_owned(), 1_500);

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        let cambiamenti = applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(cambiamenti.playlist_tolte, 1);
        let quante: i64 = connection
            .query_row("SELECT COUNT(*) FROM playlists", [], |row| row.get(0))
            .expect("conteggio");
        assert_eq!(quante, 0);
    }

    #[test]
    fn una_playlist_rinominata_non_torna_col_nome_di_prima() {
        // Il documento dell'altro dispositivo ha ancora «sera»: senza la lapide
        // della chiave vecchia la fusione la terrebbe, e qui ricomparirebbe
        // accanto a quella rinominata.
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1, 2]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let suo = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");

        let id: i64 = connection
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = 'sera'",
                [],
                |row| row.get(0),
            )
            .expect("playlist");
        let rinominata =
            crate::playlists::rename(&connection, id, "Sera estate").expect("rinominata");
        allinea(&mut connection, "portatile", 2_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 3_000),
            Documento::nuovo("telefono", suo, 3_000),
        ]);
        applica(&mut connection, &fuso, 3_000).expect("applicazione");

        let chiavi: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT playlist_key FROM playlists ORDER BY playlist_key")
                .expect("chiavi");
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .expect("chiavi")
                .map(|riga| riga.expect("riga"))
                .collect()
        };
        assert_eq!(chiavi, vec![rinominata.key.clone()]);
        assert_eq!(ordine(&connection, &rinominata.key), vec!["a", "b"]);
    }

    #[test]
    fn togliere_un_voto_viaggia_come_metterlo() {
        let mut connection = libreria();
        connection
            .execute(
                "UPDATE tracks SET rating = 5, rating_at = 1000, stats_updated_at = 1000
                  WHERE track_key = 'a'",
                [],
            )
            .expect("voto");
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = Contenuto::default();
        suo.voti.insert("a".to_owned(), Voto { v: 0, at: 2_000 });

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 3_000),
            Documento::nuovo("telefono", suo, 3_000),
        ]);
        applica(&mut connection, &fuso, 3_000).expect("applicazione");

        let stelle: i64 = connection
            .query_row(
                "SELECT rating FROM tracks WHERE track_key = 'a'",
                [],
                |row| row.get(0),
            )
            .expect("voto");
        assert_eq!(stelle, 0, "lo zero datato è una decisione, non un'assenza");
    }

    #[test]
    fn un_brano_che_qui_non_c_e_resta_nella_playlist_e_torna_quando_arriva() {
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = mio.clone();
        if let Some(playlist) = suo.playlist.get_mut("sera") {
            playlist.sequenza.accoda("telefono", "sconosciuto");
            playlist.sequenza.accoda("telefono", "b");
            playlist.at = 1_500;
        }

        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        assert_eq!(
            ordine(&connection, "sera"),
            vec!["a", "b"],
            "il brano assente si salta"
        );

        // Una passata dopo, prima che il file arrivi: l'`allinea` con cui
        // comincia ogni passata (e che gira anche dentro `applica_se_ferma`).
        // Per `playlist_tracks` il brano non c'è, ma non è stato **tolto**: la
        // sua voce non si seppellisce. Il test passava solo perché fra
        // l'applicazione e l'arrivo del file non chiamava mai `allinea`, che è
        // ciò che fa ogni passata vera.
        allinea(&mut connection, "portatile", 2_500).expect("allineamento");
        assert_eq!(ordine(&connection, "sera"), vec!["a", "b"]);
        assert_eq!(
            sequenza_di(&connection, "sera").ordine(),
            vec!["a", "sconosciuto", "b"],
            "la voce resta viva nella sequenza"
        );

        // Arriva il file.
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified)
                 VALUES (9, '/musica/x.flac', 'sconosciuto', 'x', 'chi', 'dove', 0, 0, 0)",
                [],
            )
            .expect("brano arrivato");

        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[Documento::nuovo("portatile", mio, 3_000)]);
        applica(&mut connection, &fuso, 3_000).expect("applicazione");

        assert_eq!(
            ordine(&connection, "sera"),
            vec!["a", "sconosciuto", "b"],
            "torna al suo posto, non in fondo"
        );

        // E con un `allinea` in mezzo anche fra l'arrivo e l'applicazione: lo
        // stesso brano, arrivato per un'altra via, sta già al suo posto e un
        // altro allineamento non lo muove.
        allinea(&mut connection, "portatile", 3_500).expect("allineamento");
        assert_eq!(ordine(&connection, "sera"), vec!["a", "sconosciuto", "b"]);
    }

    #[test]
    fn un_brano_che_qui_non_c_e_torna_al_suo_posto_anche_se_arriva_dopo_un_allinea() {
        // Come sopra, ma senza l'`applica` dopo l'arrivo: a riportare il brano
        // nella playlist è l'`allinea` della passata dopo.
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1]);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = mio.clone();
        if let Some(playlist) = suo.playlist.get_mut("sera") {
            playlist.sequenza.accoda("telefono", "sconosciuto");
            playlist.sequenza.accoda("telefono", "b");
            playlist.at = 1_500;
        }
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");
        allinea(&mut connection, "portatile", 2_500).expect("allineamento");

        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                     date_added, date_modified)
                 VALUES (9, '/musica/x.flac', 'sconosciuto', 'x', 'chi', 'dove', 0, 0, 0)",
                [],
            )
            .expect("brano arrivato");
        allinea(&mut connection, "portatile", 3_000).expect("allineamento");

        assert_eq!(ordine(&connection, "sera"), vec!["a", "sconosciuto", "b"]);
    }

    #[test]
    fn le_cartelle_del_cloud_viaggiano_come_sono() {
        let mut qui = libreria();
        let nuvola = "nuvola://dropbox-abcdefgh/Musica";
        settings::write_json(&qui, CHIAVE_CARTELLE, &vec!["C:\\Musica", nuvola]).expect("cartelle");
        allinea(&mut qui, "portatile", 1_000).expect("allineamento");
        let mio =
            contenuto(&qui, "portatile", BTreeMap::new(), BTreeMap::new()).expect("contenuto");
        assert!(mio.cartelle.contains_key(nuvola));

        let mut la = libreria();
        allinea(&mut la, "telefono", 1_000).expect("allineamento");
        let suo = contenuto(&la, "telefono", BTreeMap::new(), BTreeMap::new()).expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut la, &fuso, 2_000).expect("applicazione");
        let cartelle: Vec<String> = settings::read_json(&la, CHIAVE_CARTELLE)
            .expect("cartelle")
            .unwrap_or_default();
        assert!(cartelle.iter().any(|c| c == nuvola), "{cartelle:?}");
    }

    #[test]
    fn una_cartella_tolta_resta_tolta() {
        let mut connection = libreria();
        settings::write_json(
            &connection,
            CHIAVE_CARTELLE,
            &vec!["C:\\Musica", "D:\\Altro"],
        )
        .expect("cartelle");
        allinea(&mut connection, "portatile", 1_000).expect("primo allineamento");

        // Di là qualcuno la toglie.
        let mut suo = Contenuto::default();
        suo.cartelle.insert(
            "D:\\Altro".to_owned(),
            Interruttore {
                on: false,
                at: 1_500,
            },
        );
        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");

        let cartelle: Vec<String> = settings::read_json(&connection, CHIAVE_CARTELLE)
            .expect("cartelle")
            .unwrap_or_default();
        assert_eq!(cartelle, vec!["C:\\Musica".to_owned()]);

        // E non torna al giro dopo, che è il vero difetto di un elenco unito.
        allinea(&mut connection, "portatile", 2_500).expect("secondo allineamento");
        let cartelle: Vec<String> = settings::read_json(&connection, CHIAVE_CARTELLE)
            .expect("cartelle")
            .unwrap_or_default();
        assert_eq!(cartelle, vec!["C:\\Musica".to_owned()]);
    }

    /// Il documento di qui, allineato con «sera» = `a`, e il fuso con un altro
    /// dispositivo che ha messo `c` in coda.
    fn passata_su_sera(connection: &mut Connection) -> (Contenuto, Fuso) {
        playlist_con(connection, "sera", &[1]);
        allinea(connection, "portatile", 1_000).expect("allineamento");
        let mio = contenuto(connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = mio.clone();
        if let Some(playlist) = suo.playlist.get_mut("sera") {
            playlist.sequenza.accoda("telefono", "c");
            playlist.at = 1_500;
        }
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio.clone(), 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        (mio, fuso)
    }

    #[test]
    fn senza_cambiamenti_in_mezzo_si_applica_come_prima() {
        let mut connection = libreria();
        let (mio, fuso) = passata_su_sera(&mut connection);
        let esito = applica_se_ferma(&mut connection, &mio, &fuso, 2_000).expect("applicazione");
        assert!(esito.is_some_and(|cambiamenti| cambiamenti.playlist == 1));
        assert_eq!(ordine(&connection, "sera"), vec!["a", "c"]);
    }

    #[test]
    fn un_brano_aggiunto_durante_la_passata_non_si_perde() {
        let mut connection = libreria();
        let (mio, fuso) = passata_su_sera(&mut connection);

        // Mentre la passata è in rete, di qua si aggiunge `b` per la via normale.
        let id: i64 = connection
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = 'sera'",
                [],
                |row| row.get(0),
            )
            .expect("playlist");
        connection
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, 2, 1)",
                [id],
            )
            .expect("aggiunta locale");

        let esito = applica_se_ferma(&mut connection, &mio, &fuso, 2_000).expect("applicazione");
        assert_eq!(esito, None, "la libreria è cambiata: non si scrive");
        assert_eq!(
            ordine(&connection, "sera"),
            vec!["a", "b"],
            "l'aggiunta locale resta dov'è"
        );
    }

    #[test]
    fn una_cartella_aggiunta_durante_la_passata_non_si_perde() {
        let mut connection = libreria();
        settings::write_json(&connection, CHIAVE_CARTELLE, &vec!["C:\\Musica"]).expect("cartelle");
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut suo = Contenuto::default();
        suo.cartelle.insert(
            "E:\\Concerti".to_owned(),
            Interruttore {
                on: true,
                at: 1_500,
            },
        );
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio.clone(), 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);

        // Intanto, di qua, si aggiunge una cartella.
        settings::write_json(
            &connection,
            CHIAVE_CARTELLE,
            &vec!["C:\\Musica", "D:\\Nuova"],
        )
        .expect("cartelle");

        let esito = applica_se_ferma(&mut connection, &mio, &fuso, 2_000).expect("applicazione");
        assert_eq!(esito, None);
        let cartelle: Vec<String> = settings::read_json(&connection, CHIAVE_CARTELLE)
            .expect("cartelle")
            .unwrap_or_default();
        assert_eq!(
            cartelle,
            vec!["C:\\Musica".to_owned(), "D:\\Nuova".to_owned()],
            "la cartella nuova resta: quella dell'altro arriva alla passata dopo"
        );
    }

    #[test]
    fn i_dispositivi_visti_finiscono_nell_elenco() {
        let mut connection = libreria();
        let fuso = fondi(&[
            Documento::nuovo("portatile", Contenuto::default(), 1_000),
            Documento::nuovo("telefono", Contenuto::default(), 1_000),
        ]);
        applica(&mut connection, &fuso, 1_000).expect("applicazione");

        let elenco = dispositivi(&connection).expect("dispositivi");
        assert_eq!(elenco.len(), 2);
        assert!(
            elenco.iter().all(|d| !d.fidato),
            "nessuno è fidato per il solo fatto di esistere"
        );

        fidati(&connection, "telefono", Some("Il telefono"), None).expect("fiducia");
        let elenco = dispositivi(&connection).expect("dispositivi");
        let telefono = elenco
            .iter()
            .find(|d| d.id == "telefono")
            .expect("telefono");
        assert!(telefono.fidato);
        assert_eq!(telefono.nome.as_deref(), Some("Il telefono"));

        assert!(dimentica(&connection, "telefono").expect("dimenticato"));
        assert_eq!(dispositivi(&connection).expect("dispositivi").len(), 1);
    }

    #[test]
    fn rinominare_non_rende_fidato() {
        // R13-3: «Rinomina» su un dispositivo «da accettare» passava dallo
        // stesso comando di «Accetta», e da lì i suoi documenti valevano. Dare
        // un nome riconoscibile prima di decidere non è decidere.
        let connection = libreria();
        intravisti(&connection, &["sconosciuto".to_owned()], 1_000).expect("intravisto");
        assert!(rinomina(&connection, "sconosciuto", Some("Il vecchio telefono")).expect("nome"));
        let elenco = dispositivi(&connection).expect("dispositivi");
        let lui = elenco
            .iter()
            .find(|d| d.id == "sconosciuto")
            .expect("il dispositivo");
        assert_eq!(lui.nome.as_deref(), Some("Il vecchio telefono"));
        assert!(!lui.fidato, "rinominare non è accettare");

        // E a uno fidato non toglie la fiducia; un nome vuoto torna all'id.
        fidati(&connection, "sconosciuto", None, None).expect("fiducia");
        assert!(rinomina(&connection, "sconosciuto", Some("  ")).expect("nome"));
        let elenco = dispositivi(&connection).expect("dispositivi");
        let lui = elenco
            .iter()
            .find(|d| d.id == "sconosciuto")
            .expect("il dispositivo");
        assert!(lui.fidato);
        assert_eq!(lui.nome, None);
        assert!(
            !rinomina(&connection, "mai-visto", Some("x")).expect("niente"),
            "un dispositivo che non c'è non nasce dandogli un nome"
        );
    }

    #[test]
    fn lo_specchio_del_cloud_non_viaggia_con_la_sincronia() {
        // Una cache di questo PC: l'altro dispositivo rielenca da sé.
        let mut connection = libreria();
        playlist_con(&connection, "sera", &[1, 2]);
        let segreti = crate::nuvola::specchio_da_non_portare(&connection);
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let json =
            serde_json::to_string(&Documento::nuovo("portatile", mio, 2_000)).expect("in JSON");
        assert!(json.contains("sera"), "il documento c'è");
        for segreto in &segreti {
            assert!(
                !json.contains(segreto.as_str()),
                "«{segreto}» viaggerebbe col documento"
            );
        }
    }

    #[test]
    fn i_pc_associati_e_le_chiavi_di_insieme_non_viaggiano_con_la_sincronia() {
        let mut connection = libreria();
        // Una playlist, perché il documento abbia qualcosa da dire.
        playlist_con(&connection, "sera", &[1, 2]);
        let chiave = "5e".repeat(32);
        crate::insieme::associa(&connection, &chiave, "Portatile-di-Zaira-7Q", 1_000)
            .expect("associato");
        crate::insieme::salva_attivo(&connection, true).expect("acceso");
        crate::insieme::salva_nome(&connection, "Fisso-in-mansarda-K2").expect("nome");
        crate::insieme::salva_indirizzi(&connection, &["10.20.30.40:47812".to_owned()])
            .expect("indirizzi");

        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let mio = contenuto(&connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        // Il documento com'è prima della compressione: cercare dentro i byte
        // compressi passerebbe sempre, e non direbbe niente.
        let documento = Documento::nuovo("portatile", mio.clone(), 2_000);
        let json = serde_json::to_string(&documento).expect("in JSON");
        assert!(
            json.contains("sera"),
            "il documento c'è, e porta la playlist"
        );
        for segreto in [
            chiave.as_str(),
            "Portatile-di-Zaira-7Q",
            "Fisso-in-mansarda-K2",
            "10.20.30.40",
            "insieme",
        ] {
            assert!(
                !json.contains(segreto),
                "«{segreto}» viaggerebbe col documento"
            );
        }

        // I PC associati non sono dispositivi della sincronia: due fiducie,
        // due tabelle.
        assert!(
            dispositivi(&connection)
                .expect("dispositivi")
                .iter()
                .all(|d| d.id != chiave && d.nome.as_deref() != Some("Portatile-di-Zaira-7Q"))
        );

        // E l'altro verso: una passata che applica un documento venuto da
        // fuori non tocca né i PC associati né le impostazioni di qui.
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", Contenuto::default(), 2_000),
        ]);
        applica(&mut connection, &fuso, 2_000).expect("applicazione");
        assert_eq!(crate::insieme::pari(&connection).expect("pari").len(), 1);
        let impostazioni = crate::insieme::impostazioni(&connection).expect("impostazioni");
        assert!(impostazioni.attivo);
        assert_eq!(impostazioni.indirizzi, ["10.20.30.40:47812"]);
    }

    // ── le cartelle di rete che un documento porta ──

    fn cartelle_con(fuori: &[&str]) -> Contenuto {
        let mut suo = Contenuto::default();
        for percorso in fuori {
            suo.cartelle.insert(
                (*percorso).to_owned(),
                Interruttore {
                    on: true,
                    at: 1_500,
                },
            );
        }
        suo
    }

    fn applica_cartelle_di(connection: &mut Connection, suo: Contenuto) -> Cambiamenti {
        allinea(connection, "portatile", 1_000).expect("allineamento");
        let mio = contenuto(connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let fuso = fondi(&[
            Documento::nuovo("portatile", mio, 2_000),
            Documento::nuovo("telefono", suo, 2_000),
        ]);
        applica(connection, &fuso, 2_000).expect("applicazione")
    }

    fn cartelle_di(connection: &Connection) -> Vec<String> {
        settings::read_json(connection, CHIAVE_CARTELLE)
            .expect("cartelle")
            .unwrap_or_default()
    }

    #[test]
    fn una_cartella_di_rete_di_un_host_che_questo_pc_non_usa_non_entra() {
        // Solo impostazioni: nessuna di queste cartelle si chiede al disco.
        let mut qui = libreria();
        settings::write_json(&qui, CHIAVE_CARTELLE, &vec!["C:\\Musica"]).expect("cartelle");
        let di_rete = [
            r"\\192.0.2.1\m",
            "//192.0.2.1/m",
            r"\\?\UNC\192.0.2.1\m",
            r"\\192.0.2.1@SSL@443\DavWWWRoot\m",
            r"\\nas\musica",
            // R02-1: Win32 tratta come UNC qualunque coppia di separatori in
            // testa, e `\??\UNC\` è lo stesso share nello spazio dei nomi NT.
            r"/\192.0.2.1\m",
            r"\/192.0.2.1/m",
            r"\??\UNC\192.0.2.1\m",
            r"  \\192.0.2.1\m",
        ];
        let locali = [
            "D:\\Altra",
            r"\\?\E:\Musica",
            "nuvola://dropbox-abcdefgh/Musica",
            "/storage/emulated/0/Music",
        ];
        let tutte: Vec<&str> = di_rete.iter().chain(locali.iter()).copied().collect();

        let cambiamenti = applica_cartelle_di(&mut qui, cartelle_con(&tutte));

        let cartelle = cartelle_di(&qui);
        for percorso in di_rete {
            assert!(
                !cartelle.iter().any(|c| c == percorso),
                "{percorso}: {cartelle:?}"
            );
        }
        for percorso in locali {
            assert!(
                cartelle.iter().any(|c| c == percorso),
                "{percorso}: {cartelle:?}"
            );
        }
        assert_eq!(cambiamenti.cartelle_di_rete, di_rete.len());
        // E non entrano nemmeno nella mappa: il documento di qui non le
        // ripubblica, né accese né spente.
        let mappa: BTreeMap<String, Interruttore> =
            settings::read_json(&qui, CHIAVE_CARTELLE_SINCRONIA)
                .expect("mappa")
                .unwrap_or_default();
        assert!(di_rete.iter().all(|p| !mappa.contains_key(*p)), "{mappa:?}");

        // La passata dopo non cambia niente.
        let di_nuovo = applica_cartelle_di(&mut qui, cartelle_con(&tutte));
        assert_eq!(di_nuovo.cartelle, 0);
        assert_eq!(cartelle_di(&qui), cartelle);
    }

    #[test]
    fn una_cartella_di_rete_di_un_host_gia_usato_qui_entra() {
        let mut qui = libreria();
        settings::write_json(&qui, CHIAVE_CARTELLE, &vec![r"\\nas\musica"]).expect("cartelle");
        settings::write(&qui, "download.folder", r"\\archivio\scaricati").expect("download");

        let cambiamenti = applica_cartelle_di(
            &mut qui,
            cartelle_con(&[r"\\NAS\musica\Rock", r"\\archivio\musica"]),
        );

        let cartelle = cartelle_di(&qui);
        assert!(
            cartelle.iter().any(|c| c == r"\\NAS\musica\Rock"),
            "{cartelle:?}"
        );
        assert!(
            cartelle.iter().any(|c| c == r"\\archivio\musica"),
            "{cartelle:?}"
        );
        assert_eq!(cambiamenti.cartelle_di_rete, 0);
    }

    // ── i valori fuori dominio, dalla cartella alla libreria ──

    /// Una passata vera: il documento di «telefono» scritto in una cartella
    /// come la scriverebbe chiunque possa scriverci, il motore che lo legge, e
    /// la libreria che applica il fuso.
    fn passata_con(
        connection: &mut Connection,
        suo: Contenuto,
        adesso_ms: i64,
    ) -> Result<Cambiamenti, AppError> {
        use rymembra_sync::Magazzino as _;
        let dove = tempfile::tempdir().expect("cartella");
        let magazzino = rymembra_sync::Cartella::nuova(dove.path().join("dispositivi"));
        let byte =
            rymembra_sync::documento::serializza(&Documento::nuovo("telefono", suo, adesso_ms))
                .expect("serializza");
        magazzino
            .scrivi(
                &rymembra_sync::documento::nome_file("telefono"),
                None,
                "application/gzip",
                &byte,
                "x",
            )
            .expect("scrive");

        allinea(connection, "portatile", adesso_ms).expect("allineamento");
        let mio = contenuto(connection, "portatile", BTreeMap::new(), BTreeMap::new())
            .expect("contenuto");
        let mut memoria = rymembra_sync::Memoria::nuova();
        let esito = rymembra_sync::Motore::nuovo(&magazzino, "portatile", &mut memoria)
            .passata(&mio, adesso_ms, &|_| {})
            .expect("passata");
        applica(connection, &esito.fuso, adesso_ms)
    }

    #[test]
    fn un_voto_fuori_scala_non_ferma_la_passata() {
        // Sei stelle violano il `CHECK` di `tracks.rating`: la transazione
        // intera cadeva, a ogni passata, finché il file restava nella cartella.
        let mut connection = libreria();
        let mut suo = Contenuto::default();
        suo.voti.insert("a".to_owned(), Voto { v: 6, at: 1_500 });
        suo.voti.insert("b".to_owned(), Voto { v: 4, at: 1_500 });

        passata_con(&mut connection, suo, 2_000).expect("la passata si applica");
        let voti: Vec<i64> = connection
            .prepare("SELECT rating FROM tracks ORDER BY id")
            .expect("voti")
            .query_map([], |row| row.get(0))
            .expect("voti")
            .collect::<Result<_, _>>()
            .expect("voti");
        assert_eq!(voti, vec![0, 4, 0], "il voto vero arriva, quello storto no");
    }

    #[test]
    fn un_conteggio_enorme_non_fa_traboccare_la_somma() {
        // `SUM` di SQLite su interi dà «integer overflow» invece di saturare:
        // un conteggio presso `i64::MAX` accanto a un ascolto di qui faceva
        // fallire ogni passata, e poi ogni ascolto di quel brano.
        let mut connection = libreria();
        allinea(&mut connection, "portatile", 1_000).expect("allineamento");
        let tx = connection.transaction().expect("transazione");
        conta_ascolto(&tx, 1, "portatile").expect("ascolto");
        tx.commit().expect("commit");
        let mut suo = Contenuto::default();
        suo.ascolti.insert("a".to_owned(), i64::MAX);
        suo.ascolti.insert("b".to_owned(), 3);

        passata_con(&mut connection, suo, 2_000).expect("la passata si applica");
        assert_eq!(conteggio(&connection, "a"), 1);
        assert_eq!(conteggio(&connection, "b"), 3);
        let tx = connection.transaction().expect("transazione");
        conta_ascolto(&tx, 1, "portatile").expect("e il brano si ascolta ancora");
        tx.commit().expect("commit");
        assert_eq!(conteggio(&connection, "a"), 2);
    }

    #[test]
    fn un_cuore_dal_futuro_non_congela_quello_di_qui() {
        // Un cuore tolto «fra dieci anni» vinceva su ogni decisione presa qui
        // fino ad allora, e la copia che gli altri ripubblicano lo rimetteva.
        let mut connection = libreria();
        connection
            .execute(
                "UPDATE tracks SET liked = 1, liked_at = 1000 WHERE track_key = 'a'",
                [],
            )
            .expect("cuore di qui");
        let dieci_anni = 10 * 365 * 24 * 60 * 60 * 1000_i64;
        let mut suo = Contenuto::default();
        suo.preferiti.insert(
            "a".to_owned(),
            Interruttore {
                on: false,
                at: 2_000 + dieci_anni,
            },
        );

        passata_con(&mut connection, suo, 2_000).expect("la passata si applica");
        let (cuore, quando): (i64, i64) = connection
            .query_row(
                "SELECT liked, liked_at FROM tracks WHERE track_key = 'a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("cuore");
        assert_eq!((cuore, quando), (1, 1_000), "il cuore di qui resta");
    }

    // ── i brani che un dispositivo non ha (FSYNC) ──
    //
    // Una voce di sequenza il cui brano qui non c'è è invisibile qui, non
    // tolta: non si seppellisce, non si pubblica come lapide, e torna in
    // `playlist_tracks` al suo posto quando il brano arriva.

    /// Un brano di prova. `content_key` solo dove serve il trigger della 028.
    fn brano_di_prova(connection: &Connection, id: i64, chiave: &str, content_key: Option<&str>) {
        connection
            .execute(
                "INSERT INTO tracks (id, path, track_key, content_key, title, artist, album,
                                     file_size, date_added, date_modified)
                 VALUES (?1, ?2, ?3, ?4, ?3, 'chi', 'dove', 0, 0, 0)",
                rusqlite::params![id, format!("/musica/{chiave}.flac"), chiave, content_key],
            )
            .expect("brano");
    }

    /// Una libreria con questi brani, numerati da 1 nell'ordine dato.
    fn libreria_con(chiavi: &[&str]) -> Connection {
        let connection = db::open_in_memory()
            .expect("database in memoria")
            .connection;
        for (indice, chiave) in chiavi.iter().enumerate() {
            let id = i64::try_from(indice).expect("indice") + 1;
            brano_di_prova(&connection, id, chiave, None);
        }
        connection
    }

    fn id_del_brano(connection: &Connection, chiave: &str) -> i64 {
        connection
            .query_row(
                "SELECT id FROM tracks WHERE track_key = ?1",
                [chiave],
                |row| row.get(0),
            )
            .expect("brano")
    }

    /// La sequenza di una playlist com'è scritta nel database.
    fn sequenza_di(connection: &Connection, chiave: &str) -> Sequenza {
        sequenze_salvate(connection)
            .expect("sequenze")
            .remove(chiave)
            .unwrap_or_else(Sequenza::vuota)
    }

    fn documento_di(connection: &Connection, chi: &str) -> Contenuto {
        contenuto(connection, chi, BTreeMap::new(), BTreeMap::new()).expect("contenuto")
    }

    /// Una passata vera fra `qui` e il documento di un altro: allinea, legge,
    /// fonde, applica solo se la libreria è rimasta ferma.
    fn passata_con_altro(
        qui: &mut Connection,
        io: &str,
        altro: (&str, Contenuto),
        adesso_ms: i64,
    ) -> Cambiamenti {
        allinea(qui, io, adesso_ms).expect("allineamento");
        let mio = documento_di(qui, io);
        let fuso = fondi(&[
            Documento::nuovo(io, mio.clone(), adesso_ms),
            Documento::nuovo(altro.0, altro.1, adesso_ms),
        ]);
        applica_se_ferma(qui, &mio, &fuso, adesso_ms)
            .expect("applicazione")
            .expect("la libreria era ferma")
    }

    /// Il computer ha `a`, `x`, `b` e la playlist «sera» con tutti e tre; il
    /// telefono ha solo `a` e `b`, e ha ricevuto «sera» dal computer.
    fn computer_e_telefono() -> (Connection, Connection) {
        let mut computer = libreria_con(&["a", "x", "b"]);
        let mut telefono = libreria_con(&["a", "b"]);
        playlist_con(&computer, "sera", &[1, 2, 3]);
        allinea(&mut computer, "computer", 1_000).expect("allineamento");
        let del_computer = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", del_computer), 2_000);
        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "b"],
            "il punto di partenza"
        );
        (computer, telefono)
    }

    #[test]
    fn il_giro_fra_due_librerie_non_toglie_agli_altri_il_brano_che_qui_manca() {
        // Il reperto di FPROF. Il telefono riceve «sera» = a, x, b dal computer
        // e `x` non ce l'ha: la voce non sta in `playlist_tracks` (il `JOIN` su
        // `tracks` la lascia fuori), e il primo `allinea` dopo la sotterrava
        // come «tolto qui». La lapide partiva col documento successivo, e per
        // la regola di fusione («la più vecchia vince») il computer toglieva `x`
        // dalla **sua** playlist.
        let (mut computer, mut telefono) = computer_e_telefono();

        // Un'altra passata sul telefono: l'`allinea` di apertura.
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        let di_ritorno = documento_di(&telefono, "telefono");

        // E il documento del telefono torna al computer.
        let del_computer = documento_di(&computer, "computer");
        let fuso = fondi(&[
            Documento::nuovo("computer", del_computer, 4_000),
            Documento::nuovo("telefono", di_ritorno.clone(), 4_000),
        ]);
        applica(&mut computer, &fuso, 4_000).expect("applicazione");
        assert_eq!(
            ordine(&computer, "sera"),
            vec!["a", "x", "b"],
            "la playlist del computer non perde un brano che il telefono non ha"
        );
        assert_eq!(
            di_ritorno.playlist["sera"].sequenza.ordine(),
            vec!["a", "x", "b"],
            "il telefono pubblica la voce di `x` ancora viva"
        );
    }

    #[test]
    fn il_brano_che_arriva_dopo_torna_al_suo_posto_con_l_allineamento() {
        let (_computer, mut telefono) = computer_e_telefono();
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");

        // Arriva il file (la scansione, la sorveglianza, «Scarica qui»: da
        // qualunque via entri in `tracks`), e la passata dopo parte.
        brano_di_prova(&telefono, 9, "x", None);
        allinea(&mut telefono, "telefono", 4_000).expect("allineamento");

        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "x", "b"],
            "torna al suo posto, non in fondo e non sepolto"
        );
        let viva = sequenza_di(&telefono, "sera");
        assert_eq!(viva.ordine(), vec!["a", "x", "b"]);
        assert_eq!(
            viva.elementi()
                .filter(|(_, elemento)| elemento.tolto.is_some())
                .count(),
            0,
            "nessuna lapide: il brano non è mai stato tolto"
        );
    }

    #[test]
    fn la_memoria_dei_brani_assenti_si_scrive_solo_se_serve_e_si_svuota_quando_arrivano() {
        // Chi ha tutta la musica non ha mai la chiave.
        let mut completa = libreria_con(&["a", "b"]);
        playlist_con(&completa, "sera", &[1, 2]);
        allinea(&mut completa, "portatile", 1_000).expect("allineamento");
        assert_eq!(settings::read(&completa, CHIAVE_BRANI_ASSENTI), Ok(None));

        // Il telefono ha ricevuto «sera» con `x`, che non ha.
        let (_computer, mut telefono) = computer_e_telefono();
        assert_eq!(
            settings::read_json::<BTreeSet<String>>(&telefono, CHIAVE_BRANI_ASSENTI),
            Ok(Some(BTreeSet::from(["x".to_owned()])))
        );

        // Arriva, e la prima passata lo aggancia e lo toglie dalla memoria.
        brano_di_prova(&telefono, 9, "x", None);
        let fatto = allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        assert_eq!(fatto.agganciate, 1);
        assert!(!fatto.e_vuoto());
        assert_eq!(settings::read(&telefono, CHIAVE_BRANI_ASSENTI), Ok(None));

        // E la passata dopo non ha più niente da fare.
        let ancora = allinea(&mut telefono, "telefono", 4_000).expect("allineamento");
        assert!(ancora.e_vuoto(), "{ancora:?}");
    }

    #[test]
    fn la_voce_invisibile_sopravvive_a_piu_allineamenti_di_fila() {
        let (_computer, mut telefono) = computer_e_telefono();
        let prima = sequenza_di(&telefono, "sera");
        assert_eq!(prima.ordine(), vec!["a", "x", "b"]);

        for volta in 0..4 {
            let fatto = allinea(&mut telefono, "telefono", 3_000 + volta).expect("allineamento");
            assert_eq!(
                fatto.sequenze, 0,
                "la {volta}ª passata non ha niente da rifare"
            );
            assert_eq!(
                sequenza_di(&telefono, "sera"),
                prima,
                "la sequenza resta identica: né elementi nuovi né lapidi ({volta})"
            );
            assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);
        }
    }

    #[test]
    fn un_brano_tolto_davvero_dalla_playlist_si_seppellisce_e_si_propaga() {
        // La guardia: la regola nuova non deve far tornare un brano che
        // l'utente ha tolto. `b` c'è nella libreria e non c'è più nella
        // playlist: è una decisione, e viaggia come lapide.
        let mut qui = libreria_con(&["a", "b", "c"]);
        playlist_con(&qui, "sera", &[1, 2, 3]);
        allinea(&mut qui, "portatile", 1_000).expect("allineamento");
        let vecchio = documento_di(&qui, "portatile");

        qui.execute(
            "DELETE FROM playlist_tracks WHERE track_id = ?1",
            [id_del_brano(&qui, "b")],
        )
        .expect("tolto dalla playlist");
        allinea(&mut qui, "portatile", 2_000).expect("allineamento");

        let sequenza = sequenza_di(&qui, "sera");
        assert_eq!(sequenza.ordine(), vec!["a", "c"]);
        assert_eq!(
            sequenza
                .elementi()
                .filter(|(_, elemento)| elemento.tolto == Some(2_000))
                .map(|(_, elemento)| elemento.brano.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );

        // L'altro dispositivo aveva ancora i tre: la lapide vince.
        let fuso = fondi(&[
            Documento::nuovo("portatile", documento_di(&qui, "portatile"), 3_000),
            Documento::nuovo("telefono", vecchio, 3_000),
        ]);
        assert_eq!(fuso.playlist["sera"].sequenza.ordine(), vec!["a", "c"]);
    }

    #[test]
    fn un_file_cancellato_qui_non_toglie_il_brano_dalle_playlist_degli_altri() {
        // Due librerie con gli stessi tre brani e la stessa playlist, già in
        // pari. Su una il file di `x` sparisce (disco staccato, cartella
        // spostata, «Togli dalla libreria»): la cascata porta via la riga di
        // `playlist_tracks`, ma è un fatto locale, non una decisione.
        let mut computer = libreria_con(&["a", "x", "b"]);
        let mut telefono = libreria_con(&["a", "x", "b"]);
        playlist_con(&computer, "sera", &[1, 2, 3]);
        allinea(&mut computer, "computer", 1_000).expect("allineamento");
        let del_computer = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", del_computer), 2_000);
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "x", "b"]);

        telefono
            .execute("DELETE FROM tracks WHERE track_key = 'x'", [])
            .expect("file cancellato");
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);

        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        allinea(&mut telefono, "telefono", 3_500).expect("allineamento");
        let del_telefono = documento_di(&telefono, "telefono");
        assert_eq!(
            del_telefono.playlist["sera"].sequenza.ordine(),
            vec!["a", "x", "b"],
            "il telefono non pubblica una lapide per un file che non ha più"
        );
        let fuso = fondi(&[
            Documento::nuovo("computer", documento_di(&computer, "computer"), 4_000),
            Documento::nuovo("telefono", del_telefono, 4_000),
        ]);
        applica(&mut computer, &fuso, 4_000).expect("applicazione");
        assert_eq!(ordine(&computer, "sera"), vec!["a", "x", "b"]);

        // Il file torna (la scansione lo reinserisce, con un id nuovo).
        brano_di_prova(&telefono, 20, "x", None);
        allinea(&mut telefono, "telefono", 5_000).expect("allineamento");
        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "x", "b"],
            "e con lui torna al suo posto"
        );
    }

    #[test]
    fn un_file_che_torna_con_la_sua_chiave_di_contenuto_non_si_raddoppia() {
        // Il trigger della 028 rimette il posto in `playlist_tracks` quando
        // torna un file con la stessa `content_key`. L'aggancio non deve
        // aggiungerne un secondo.
        let mut telefono = libreria_con(&[]);
        for (id, chiave) in [(1, "a"), (2, "x"), (3, "b")] {
            brano_di_prova(&telefono, id, chiave, Some(&format!("c-{chiave}")));
        }
        playlist_con(&telefono, "sera", &[1, 2, 3]);
        allinea(&mut telefono, "telefono", 1_000).expect("allineamento");

        telefono
            .execute("DELETE FROM tracks WHERE track_key = 'x'", [])
            .expect("file cancellato");
        allinea(&mut telefono, "telefono", 2_000).expect("allineamento");
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);

        brano_di_prova(&telefono, 30, "x", Some("c-x"));
        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "x", "b"],
            "il trigger ha già rimesso il brano"
        );
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "x", "b"]);
        assert_eq!(
            sequenza_di(&telefono, "sera")
                .elementi()
                .filter(|(_, elemento)| elemento.tolto.is_none())
                .count(),
            3,
            "tre voci vive, una per posto"
        );
    }

    #[test]
    fn un_brano_arrivato_e_aggiunto_a_mano_prima_dell_allineamento_non_si_raddoppia() {
        let (_computer, mut telefono) = computer_e_telefono();
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");

        brano_di_prova(&telefono, 9, "x", None);
        // L'utente lo aggiunge in fondo, per conto suo, prima che passi
        // l'allineamento.
        telefono
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 SELECT id, 9, 2 FROM playlists WHERE playlist_key = 'sera'",
                [],
            )
            .expect("aggiunta a mano");
        allinea(&mut telefono, "telefono", 4_000).expect("allineamento");

        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "b", "x"],
            "l'ordine che l'utente ha fatto qui vale, e `x` c'è una volta"
        );
        assert_eq!(sequenza_di(&telefono, "sera").ordine(), vec!["a", "b", "x"]);
    }

    #[test]
    fn un_riordino_fatto_qui_non_tocca_la_voce_di_un_brano_che_qui_manca() {
        let (_computer, mut telefono) = computer_e_telefono();
        let id_a = id_del_brano(&telefono, "a");
        let id_b = id_del_brano(&telefono, "b");
        // L'utente mette `b` prima di `a`.
        telefono
            .execute_batch(&format!(
                "DELETE FROM playlist_tracks;
                 INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 SELECT id, {id_b}, 0 FROM playlists WHERE playlist_key = 'sera';
                 INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 SELECT id, {id_a}, 1 FROM playlists WHERE playlist_key = 'sera';"
            ))
            .expect("riordino");
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");

        let sequenza = sequenza_di(&telefono, "sera");
        let visibili: Vec<&str> = sequenza
            .ordine()
            .into_iter()
            .filter(|brano| *brano != "x")
            .collect();
        assert_eq!(visibili, vec!["b", "a"], "l'ordine di qui si pubblica");
        assert_eq!(
            sequenza.ordine().iter().filter(|b| **b == "x").count(),
            1,
            "la voce di `x` è ancora lì, una volta, viva"
        );
        assert_eq!(ordine(&telefono, "sera"), vec!["b", "a"]);

        // Il file arriva: `x` compare, e `b`, `a` restano come li ha voluti
        // l'utente.
        brano_di_prova(&telefono, 9, "x", None);
        allinea(&mut telefono, "telefono", 4_000).expect("allineamento");
        let dopo = ordine(&telefono, "sera");
        assert_eq!(dopo.len(), 3, "{dopo:?}");
        let senza_x: Vec<&String> = dopo.iter().filter(|b| *b != "x").collect();
        assert_eq!(senza_x, vec!["b", "a"], "{dopo:?}");
    }

    #[test]
    fn la_playlist_cancellata_qui_seppellisce_tutta_la_sua_sequenza() {
        // La guardia del ramo «sequenze rimaste»: una playlist che non c'è più
        // non lascia voci vive, nemmeno quelle dei brani che qui mancano.
        let (_computer, mut telefono) = computer_e_telefono();
        telefono
            .execute("DELETE FROM playlists WHERE playlist_key = 'sera'", [])
            .expect("playlist cancellata");
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        let sequenza = sequenza_di(&telefono, "sera");
        assert!(sequenza.ordine().is_empty(), "{sequenza:?}");
        assert_eq!(sequenza.quanti(), 3, "le lapidi restano");
    }

    #[test]
    fn applica_se_ferma_si_comporta_come_allinea_e_applica() {
        // Due telefoni identici. Su uno la passata di sempre (allinea, legge,
        // fonde, applica_se_ferma); sull'altro lo stesso ma con `allinea` e
        // `applica` separati. Il brano `x` arriva fra la lettura e
        // l'applicazione, come in una passata che sta due minuti in rete.
        let (computer, telefono_uno) = computer_e_telefono();
        let (_, telefono_due) = computer_e_telefono();
        let (mut uno, mut due) = (telefono_uno, telefono_due);
        let del_computer = documento_di(&computer, "computer");

        let mio_uno = documento_di(&uno, "telefono");
        let mio_due = documento_di(&due, "telefono");
        let fuso_uno = fondi(&[
            Documento::nuovo("computer", del_computer.clone(), 3_000),
            Documento::nuovo("telefono", mio_uno.clone(), 3_000),
        ]);
        let fuso_due = fondi(&[
            Documento::nuovo("computer", del_computer, 3_000),
            Documento::nuovo("telefono", mio_due, 3_000),
        ]);
        brano_di_prova(&uno, 9, "x", None);
        brano_di_prova(&due, 9, "x", None);

        let esito = applica_se_ferma(&mut uno, &mio_uno, &fuso_uno, 3_000).expect("applicazione");
        assert!(
            esito.is_some(),
            "una sequenza rimasta com'era non ferma l'applicazione"
        );

        allinea(&mut due, "telefono", 3_000).expect("allineamento");
        applica(&mut due, &fuso_due, 3_000).expect("applicazione");

        assert_eq!(ordine(&uno, "sera"), vec!["a", "x", "b"]);
        assert_eq!(ordine(&uno, "sera"), ordine(&due, "sera"));
        assert_eq!(sequenza_di(&uno, "sera"), sequenza_di(&due, "sera"));
    }

    #[test]
    fn una_passata_che_non_si_applica_non_fa_perdere_l_aggancio_alla_passata_dopo() {
        // Il brano arriva mentre la passata è in rete e, nello stesso tempo,
        // l'utente cambia un'altra playlist: `applica_se_ferma` non scrive e la
        // sua transazione cade, aggancio compreso. La passata dopo deve
        // agganciare comunque, non sotterrare.
        let (computer, mut telefono) = computer_e_telefono();
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        let mio = documento_di(&telefono, "telefono");
        let fuso = fondi(&[
            Documento::nuovo("computer", documento_di(&computer, "computer"), 3_500),
            Documento::nuovo("telefono", mio.clone(), 3_500),
        ]);

        brano_di_prova(&telefono, 9, "x", None);
        playlist_con(&telefono, "corsa", &[1]);
        let esito = applica_se_ferma(&mut telefono, &mio, &fuso, 3_500).expect("applicazione");
        assert_eq!(esito, None, "la libreria è cambiata: non si scrive");

        allinea(&mut telefono, "telefono", 4_000).expect("allineamento");
        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "x", "b"],
            "agganciato, non sepolto"
        );
        assert_eq!(
            sequenza_di(&telefono, "sera")
                .elementi()
                .filter(|(_, elemento)| elemento.tolto.is_some())
                .count(),
            0
        );
    }

    // ── FSYNC2: la sequenza segue la playlist ──

    fn id_playlist(connection: &Connection, chiave: &str) -> i64 {
        connection
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = ?1",
                [chiave],
                |row| row.get(0),
            )
            .expect("playlist")
    }

    /// Un istante dopo ogni ora vera: le funzioni delle playlist datano con
    /// l'orologio, e una passata fatta «prima» vedrebbe quelle date dal futuro.
    const DOPO: i64 = 4_000_000_000_000;

    #[test]
    fn rinominare_la_playlist_su_un_telefono_non_toglie_al_computer_il_brano_che_il_telefono_non_ha()
     {
        // Il telefono (che non ha `x`) rinomina «sera». La chiave cambia, e la
        // sequenza — con la voce viva di `x` — deve seguirla: prima nasceva
        // da zero coi soli brani di qui, la vecchia partiva con le lapidi, e il
        // computer perdeva `x` dalla playlist col nome nuovo.
        let (mut computer, mut telefono) = computer_e_telefono();
        let id = id_playlist(&telefono, "sera");
        crate::playlists::rename(&telefono, id, "sera2").expect("rinomino");

        let del_computer = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", del_computer), DOPO);
        assert_eq!(
            sequenza_di(&telefono, "sera2").ordine(),
            vec!["a", "x", "b"],
            "la voce di `x` ha seguito la playlist"
        );
        assert!(
            sequenza_di(&telefono, "sera").e_vuota(),
            "e non è rimasta sotto il nome vecchio"
        );
        let dal_telefono = documento_di(&telefono, "telefono");

        let fuso = fondi(&[
            Documento::nuovo("computer", documento_di(&computer, "computer"), DOPO + 1),
            Documento::nuovo("telefono", dal_telefono, DOPO + 1),
        ]);
        applica(&mut computer, &fuso, DOPO + 1).expect("applicazione");
        assert_eq!(
            ordine(&computer, "sera2"),
            vec!["a", "x", "b"],
            "il computer ha ancora `x` nella playlist rinominata dal telefono"
        );
        assert_eq!(ordine(&telefono, "sera2"), vec!["a", "b"]);
    }

    #[test]
    fn rinominare_su_un_nome_cancellato_qui_non_stampa_le_vecchie_lapidi_sulla_playlist_che_arriva()
    {
        // La chiave nuova è libera perché una playlist con quel nome è stata
        // cancellata qui, e di lei restano le righe di sequenza (sepolte da un
        // `allinea`). Le identità sono quelle di ogni sequenza battezzata da una
        // lista: fondere quelle righe con la sequenza che arriva stamperebbe le
        // lapidi di una playlist che non c'è più sulle voci vive di quella
        // rinominata.
        let (_computer, mut telefono) = computer_e_telefono();
        playlist_con(&telefono, "altra", &[1]);
        allinea(&mut telefono, "telefono", DOPO).expect("allineamento");
        let vecchia = id_playlist(&telefono, "altra");
        crate::playlists::delete(&mut telefono, vecchia).expect("cancellata");
        allinea(&mut telefono, "telefono", DOPO + 1).expect("allineamento");
        assert!(
            sequenza_di(&telefono, "altra").ordine().is_empty(),
            "la vecchia è sepolta"
        );

        let id = id_playlist(&telefono, "sera");
        crate::playlists::rename(&telefono, id, "altra").expect("rinomino");

        let seguita = sequenza_di(&telefono, "altra");
        assert_eq!(seguita.ordine(), vec!["a", "x", "b"]);
        assert_eq!(
            seguita
                .elementi()
                .filter(|(_, elemento)| elemento.tolto.is_some())
                .count(),
            0,
            "nessuna lapide delle voci della playlist cancellata"
        );
    }

    // ── FSYNC2: tolto dalla playlist e poi dalla libreria, prima di un allinea ──

    /// Quel che un `allinea` può vedere di una libreria, come testo: le tabelle
    /// che la sincronia legge più quelle del trigger della 28.
    fn quel_che_si_vede(connection: &Connection) -> String {
        let mut fuori = String::new();
        for interrogazione in [
            "SELECT track_key FROM tracks ORDER BY track_key",
            "SELECT p.playlist_key, t.track_key, pt.position FROM playlist_tracks pt
               JOIN playlists p ON p.id = pt.playlist_id
               JOIN tracks t ON t.id = pt.track_id ORDER BY 1, 3",
            "SELECT playlist_key, elemento, track_key, tolto_ms FROM sync_sequenza ORDER BY 1, 2",
            "SELECT content_key, playlist_id, position FROM playlist_orfane ORDER BY 1, 2, 3",
        ] {
            let mut statement = connection.prepare(interrogazione).expect("interrogazione");
            let colonne = statement.column_count();
            let righe = statement
                .query_map([], |riga| {
                    (0..colonne)
                        .map(|i| {
                            riga.get::<_, rusqlite::types::Value>(i)
                                .map(|v| format!("{v:?}"))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .expect("righe");
            for riga in righe {
                fuori.push_str(&riga.expect("riga").join("|"));
                fuori.push('\n');
            }
            fuori.push_str("--\n");
        }
        fuori
    }

    /// Un telefono con `a`, `x`, `b` e «sera», già allineato; `con_chiave_di_contenuto`
    /// dice se i brani hanno la `content_key` che il trigger della 28 pretende.
    fn telefono_con_sera(con_chiave_di_contenuto: bool) -> Connection {
        let mut telefono = libreria_con(&[]);
        for (id, chiave) in [(1, "a"), (2, "x"), (3, "b")] {
            let ck = con_chiave_di_contenuto.then(|| format!("c-{chiave}"));
            brano_di_prova(&telefono, id, chiave, ck.as_deref());
        }
        playlist_con(&telefono, "sera", &[1, 2, 3]);
        allinea(&mut telefono, "telefono", 1_000).expect("allineamento");
        telefono
    }

    #[test]
    fn senza_chiave_di_contenuto_tolto_e_poi_cancellato_non_si_distingue_dal_solo_cancellato() {
        // Il limite, provato. Il padre di FSYNC seppelliva la voce di un brano
        // tolto dalla playlist e poi sparito dalla libreria prima di un
        // `allinea`, e il giro dopo lasciava invisibile quella di un brano che
        // sparisce soltanto — perdendo i brani agli altri dispositivi. Le due
        // storie lasciano lo STESSO stato nelle tabelle quando il trigger della
        // 28 non scrive niente (brano senza `content_key`, o senza percorso: un
        // brano di catalogo, una riga provvisoria del cloud): nessuna regola può
        // seppellire l'una senza seppellire l'altra, e l'errore che si sceglie
        // è la voce che resta.
        let tolto_e_poi_cancellato = {
            let telefono = telefono_con_sera(false);
            telefono
                .execute("DELETE FROM playlist_tracks WHERE track_id = 2", [])
                .expect("tolto dalla playlist");
            telefono
                .execute("DELETE FROM tracks WHERE track_key = 'x'", [])
                .expect("cancellato");
            telefono
        };
        let solo_cancellato = {
            let telefono = telefono_con_sera(false);
            telefono
                .execute("DELETE FROM tracks WHERE track_key = 'x'", [])
                .expect("cancellato");
            telefono
        };
        assert_eq!(
            quel_che_si_vede(&tolto_e_poi_cancellato),
            quel_che_si_vede(&solo_cancellato),
            "stesse tabelle: l'`allinea` non può dire quale delle due storie è la sua"
        );
        for mut telefono in [tolto_e_poi_cancellato, solo_cancellato] {
            allinea(&mut telefono, "telefono", 2_000).expect("allineamento");
            assert_eq!(lapidi_di(&telefono, "sera"), 0);
            assert_eq!(
                sequenza_di(&telefono, "sera").ordine(),
                vec!["a", "x", "b"],
                "la voce resta, invisibile: non parte nessuna lapide"
            );
        }
    }

    #[test]
    fn con_la_chiave_di_contenuto_il_trigger_lascia_un_segno_solo_a_chi_e_stato_cancellato_dentro_la_playlist()
     {
        // L'informazione che mancava sopra, quando c'è: `playlist_orfane` ha una
        // riga per il brano uscito mentre stava nella playlist, e niente per
        // quello tolto prima. Ma è per `content_key`, e del brano sparito non
        // resta la chiave: si sa solo che la playlist ha un posto tenuto.
        let tolto_e_poi_cancellato = {
            let telefono = telefono_con_sera(true);
            telefono
                .execute("DELETE FROM playlist_tracks WHERE track_id = 2", [])
                .expect("tolto dalla playlist");
            telefono
                .execute("DELETE FROM tracks WHERE track_key = 'x'", [])
                .expect("cancellato");
            telefono
        };
        let solo_cancellato = {
            let telefono = telefono_con_sera(true);
            telefono
                .execute("DELETE FROM tracks WHERE track_key = 'x'", [])
                .expect("cancellato");
            telefono
        };
        let posti_tenuti = |telefono: &Connection| -> i64 {
            telefono
                .query_row("SELECT COUNT(*) FROM playlist_orfane", [], |riga| {
                    riga.get(0)
                })
                .expect("conteggio")
        };
        assert_eq!(posti_tenuti(&tolto_e_poi_cancellato), 0);
        assert_eq!(posti_tenuti(&solo_cancellato), 1);
    }

    // ── FSYNC2: una playlist cancellata porta con sé la sua sequenza ──

    #[test]
    fn una_playlist_cancellata_e_rifatta_prima_di_un_allinea_non_porta_le_voci_della_vecchia() {
        // Il telefono cancella «sera» (a, x, b: `x` è un brano che non ha) e la
        // rifà con lo stesso nome e il solo `a`, senza che passi un `allinea`.
        // La voce invisibile di `x` apparteneva alla vecchia: nella nuova non
        // deve vivere, né qui né sul computer.
        let (mut computer, mut telefono) = computer_e_telefono();
        let id = id_playlist(&telefono, "sera");
        crate::playlists::delete(&mut telefono, id).expect("cancellata");
        // La lapide e la riga nuova devono avere date diverse: una playlist
        // rifatta nello stesso millisecondo non sopravvive alla propria lapide.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let a = id_del_brano(&telefono, "a");
        crate::playlists::crea_e_aggiungi(&mut telefono, "sera", &[a]).expect("rifatta");

        let del_computer = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", del_computer), DOPO);
        assert_eq!(ordine(&telefono, "sera"), vec!["a"]);
        assert_eq!(
            sequenza_di(&telefono, "sera").ordine(),
            vec!["a"],
            "la voce invisibile della vecchia non vive nella nuova"
        );

        let dal_telefono = documento_di(&telefono, "telefono");
        let fuso = fondi(&[
            Documento::nuovo("computer", documento_di(&computer, "computer"), DOPO + 1),
            Documento::nuovo("telefono", dal_telefono, DOPO + 1),
        ]);
        applica(&mut computer, &fuso, DOPO + 1).expect("applicazione");
        assert_eq!(
            ordine(&computer, "sera"),
            vec!["a"],
            "la playlist rifatta col solo `a` non porta la voce di `x` della vecchia"
        );
    }

    #[test]
    fn annullare_la_cancellazione_rida_vive_le_voci_dei_brani_che_qui_mancano() {
        // «Annulla» rimette la playlist com'era, e com'era aveva la voce
        // invisibile di `x`: sepolta dalla cancellazione, deve tornare viva, o
        // la passata dopo vedrebbe una playlist senza `x` e lo toglierebbe agli
        // altri dispositivi.
        let (_computer, mut telefono) = computer_e_telefono();
        let id = id_playlist(&telefono, "sera");
        let cancellata = crate::playlists::cancella(&mut telefono, id).expect("cancellata");
        assert!(
            sequenza_di(&telefono, "sera").ordine().is_empty(),
            "la sequenza muore con la playlist"
        );
        // Una passata in mezzo non cambia niente: la sequenza è già sepolta.
        let in_mezzo = allinea(&mut telefono, "telefono", DOPO).expect("allineamento");
        assert_eq!(in_mezzo.sequenze, 0, "{in_mezzo:?}");

        crate::playlists::ripristina(&mut telefono, &cancellata).expect("ripristinata");
        assert_eq!(
            sequenza_di(&telefono, "sera").ordine(),
            vec!["a", "x", "b"],
            "tornano vive, la voce invisibile compresa"
        );
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);

        let dopo = allinea(&mut telefono, "telefono", DOPO + 1).expect("allineamento");
        assert!(dopo.e_vuoto(), "{dopo:?}");
        assert_eq!(lapidi_di(&telefono, "sera"), 0);
    }

    // ── FSYNC2: la memoria che manca e quella che non si legge ──

    fn lapidi_di(connection: &Connection, chiave: &str) -> usize {
        sequenza_di(connection, chiave)
            .elementi()
            .filter(|(_, elemento)| elemento.tolto.is_some())
            .count()
    }

    #[test]
    fn una_memoria_illeggibile_non_fa_seppellire_il_brano_arrivato() {
        // Il telefono ha `a` e `b`, la voce di `x` è viva e invisibile. La
        // memoria dei brani assenti si guasta (un valore storto, un database
        // ripristinato da un file di prima) e `x` arriva: leggerla come «niente
        // di ricordato» vuol dire seppellire `x` e far partire la lapide.
        let (_computer, mut telefono) = computer_e_telefono();
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        settings::write(&telefono, CHIAVE_BRANI_ASSENTI, "{non json").expect("guasta");
        brano_di_prova(&telefono, 9, "x", None);

        let fatto = allinea(&mut telefono, "telefono", 4_000).expect("allineamento");
        assert_eq!(
            lapidi_di(&telefono, "sera"),
            0,
            "nessuna voce sepolta: {fatto:?}"
        );
        assert_eq!(sequenza_di(&telefono, "sera").ordine(), vec!["a", "x", "b"]);
        // L'errore sicuro è una voce che torna: `x` rientra al suo posto.
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "x", "b"]);
        assert_eq!(fatto.agganciate, 1);
        // E la memoria si è rifatta: quel che era guasto non resta lì.
        assert_eq!(settings::read(&telefono, CHIAVE_BRANI_ASSENTI), Ok(None));

        // La passata dopo non ha niente da fare, e non seppellisce niente.
        let ancora = allinea(&mut telefono, "telefono", 5_000).expect("allineamento");
        assert!(ancora.e_vuoto(), "{ancora:?}");
        assert_eq!(lapidi_di(&telefono, "sera"), 0);
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "x", "b"]);
    }

    #[test]
    fn una_memoria_illeggibile_riporta_il_brano_tolto_invece_di_seppellirlo() {
        // L'errore che si sceglie, scritto: se la memoria non si legge e
        // l'utente ha tolto `b` proprio in quella finestra, `b` torna nella
        // playlist. Una voce che torna si toglie un'altra volta; una lapide
        // che parte per sbaglio toglie un brano agli altri dispositivi.
        let mut qui = libreria_con(&["a", "b", "c"]);
        playlist_con(&qui, "sera", &[1, 2, 3]);
        allinea(&mut qui, "portatile", 1_000).expect("allineamento");
        qui.execute(
            "DELETE FROM playlist_tracks WHERE track_id = ?1",
            [id_del_brano(&qui, "b")],
        )
        .expect("tolto");
        settings::write(&qui, CHIAVE_BRANI_ASSENTI, "[1, 2").expect("guasta");

        allinea(&mut qui, "portatile", 2_000).expect("allineamento");
        assert_eq!(lapidi_di(&qui, "sera"), 0);
        assert_eq!(ordine(&qui, "sera"), vec!["a", "b", "c"]);

        // Con la memoria a posto, lo stesso gesto è una rimozione vera.
        qui.execute(
            "DELETE FROM playlist_tracks WHERE track_id = ?1",
            [id_del_brano(&qui, "b")],
        )
        .expect("tolto");
        allinea(&mut qui, "portatile", 3_000).expect("allineamento");
        assert_eq!(lapidi_di(&qui, "sera"), 1);
        assert_eq!(ordine(&qui, "sera"), vec!["a", "c"]);
    }

    #[test]
    fn una_memoria_che_manca_non_e_una_memoria_illeggibile() {
        // «Nessun ricordo» è lo stato normale: un brano tolto si seppellisce,
        // come ha sempre fatto, e non si riaggancia niente.
        let mut qui = libreria_con(&["a", "b", "c"]);
        playlist_con(&qui, "sera", &[1, 2, 3]);
        allinea(&mut qui, "portatile", 1_000).expect("allineamento");
        assert_eq!(settings::read(&qui, CHIAVE_BRANI_ASSENTI), Ok(None));
        qui.execute(
            "DELETE FROM playlist_tracks WHERE track_id = ?1",
            [id_del_brano(&qui, "b")],
        )
        .expect("tolto");
        let fatto = allinea(&mut qui, "portatile", 2_000).expect("allineamento");
        assert_eq!(fatto.agganciate, 0);
        assert_eq!(lapidi_di(&qui, "sera"), 1);
        assert_eq!(ordine(&qui, "sera"), vec!["a", "c"]);

        // Una memoria con un insieme vuoto è un ricordo valido: legge e basta.
        settings::write(&qui, CHIAVE_BRANI_ASSENTI, "[]").expect("vuota");
        assert_eq!(
            ricordo_dei_brani_assenti(&qui),
            Ok(Ricordo::Chiavi(BTreeSet::new()))
        );
        settings::write(&qui, CHIAVE_BRANI_ASSENTI, "{non json").expect("guasta");
        assert_eq!(ricordo_dei_brani_assenti(&qui), Ok(Ricordo::Illeggibile));
    }

    // ── FSYNC2: il costo di una passata ferma ──

    fn azzera_le_letture() {
        LETTURE.with(|letture| letture.set((0, 0)));
    }

    fn letture() -> (usize, usize) {
        LETTURE.with(std::cell::Cell::get)
    }

    #[test]
    fn una_passata_ferma_interroga_una_volta_sola_i_brani_assenti_e_la_memoria() {
        // Prima li ricalcolava due volte ciascuno: una per decidere e una per
        // riscrivere la memoria, tutto col lucchetto della libreria in mano.
        let (_computer, mut telefono) = computer_e_telefono();
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");

        azzera_le_letture();
        let fatto = allinea(&mut telefono, "telefono", 4_000).expect("allineamento");
        assert!(fatto.e_vuoto(), "{fatto:?}");
        assert_eq!(
            letture(),
            (1, 1),
            "una interrogazione dei brani assenti e una lettura della memoria"
        );
    }

    #[test]
    fn la_memoria_nomina_ancora_il_brano_assente_dopo_che_una_playlist_ne_ha_sepolto_la_voce() {
        // Il riuso dei brani assenti calcolati a inizio passata vale anche
        // dopo che la passata ha sepolto una playlist intera, voci dei brani
        // che mancano comprese: la memoria nomina i brani di tutti gli
        // elementi, vivi o tolti, e una lapide non ne cambia la chiave. È anche
        // quel che la tiene «appiccicosa» fino al ritorno del file.
        let (_computer, mut telefono) = computer_e_telefono();
        let con_x = Ok(Some(BTreeSet::from(["x".to_owned()])));
        assert_eq!(
            settings::read_json::<BTreeSet<String>>(&telefono, CHIAVE_BRANI_ASSENTI),
            con_x
        );
        telefono
            .execute("DELETE FROM playlists WHERE playlist_key = 'sera'", [])
            .expect("playlist cancellata");

        azzera_le_letture();
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        assert_eq!(letture(), (1, 1));
        assert_eq!(
            sequenza_di(&telefono, "sera").ordine(),
            Vec::<&str>::new(),
            "la voce di `x` è sepolta con la playlist"
        );
        assert_eq!(
            settings::read_json::<BTreeSet<String>>(&telefono, CHIAVE_BRANI_ASSENTI),
            con_x,
            "ma `x` manca ancora, e lo ricorda finché non torna"
        );
    }

    /// Una misura, non una prova: `cargo test -p rymembra-app --release --lib
    /// -- --ignored --nocapture costo_di_una_passata`. Un telefono con il 5%
    /// della musica di un computer da 50 000 brani, 200 playlist da 100 voci.
    #[test]
    #[ignore = "misura: si lancia a mano con --ignored --nocapture"]
    fn costo_di_una_passata_ferma_su_un_telefono_con_il_cinque_per_cento() {
        use std::time::Instant;
        let mut telefono = db::open_in_memory()
            .expect("database in memoria")
            .connection;
        {
            let tx = telefono.transaction().expect("transazione");
            for i in 0..2_500_i64 {
                let chiave = format!("artista {i}|titolo {i}|album dove");
                tx.execute(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size,
                                         date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?3, 'chi', 'dove', 0, 0, 0)",
                    rusqlite::params![i + 1, format!("/musica/{i}.flac"), chiave],
                )
                .expect("brano");
            }
            tx.commit().expect("chiusura");
        }
        let mut contenuto = Contenuto::default();
        let mut seme: u64 = 0x2545_F491_4F6C_DD1D;
        let mut prossimo = move || {
            seme ^= seme << 13;
            seme ^= seme >> 7;
            seme ^= seme << 17;
            seme
        };
        for p in 0..200 {
            let brani: Vec<String> = (0..100)
                .map(|_| {
                    let i = prossimo() % 50_000;
                    format!("artista {i}|titolo {i}|album dove")
                })
                .collect();
            let nome = format!("playlist {p}");
            contenuto.playlist.insert(
                nome.clone(),
                PlaylistSincronizzata {
                    nome,
                    descrizione: None,
                    creata_at: 100,
                    at: 100,
                    smart: false,
                    regole: None,
                    sequenza: Sequenza::dalla_lista("computer", &brani),
                },
            );
        }
        let fuso = fondi(&[Documento::nuovo("computer", contenuto.clone(), 1_000)]);
        let t0 = Instant::now();
        applica(&mut telefono, &fuso, 1_000).expect("applicazione");
        eprintln!(
            "COSTO applica (200 playlist, 20000 voci): {:?}",
            t0.elapsed()
        );
        let grezzo = settings::read(&telefono, CHIAVE_BRANI_ASSENTI)
            .expect("lettura")
            .unwrap_or_default();
        eprintln!("COSTO memoria: {} byte di JSON", grezzo.len());

        allinea(&mut telefono, "telefono", 2_000).expect("allineamento");
        let prima = telefono.total_changes();
        let t0 = Instant::now();
        for giro in 0..5 {
            let fatto = allinea(&mut telefono, "telefono", 3_000 + giro).expect("allineamento");
            assert!(fatto.e_vuoto(), "{fatto:?}");
        }
        eprintln!(
            "COSTO allinea su libreria ferma: {:?} a giro, righe scritte: {}",
            t0.elapsed() / 5,
            telefono.total_changes() - prima
        );

        let mio = documento_di(&telefono, "telefono");
        let fuso = fondi(&[
            Documento::nuovo("computer", contenuto, 4_000),
            Documento::nuovo("telefono", mio.clone(), 4_000),
        ]);
        let t0 = Instant::now();
        let esito = applica_se_ferma(&mut telefono, &mio, &fuso, 4_000).expect("applicazione");
        eprintln!(
            "COSTO applica_se_ferma (niente da cambiare): {:?}, applicata: {}",
            t0.elapsed(),
            esito.is_some()
        );
    }

    // ── FSYNC2: il ritorno di una voce tolta altrove ──

    /// Due dispositivi con `a`, `x`, `b` (`content_key` per il trigger della
    /// 28) e «sera» già uguale su tutti e due. Il disco del telefono si stacca
    /// e `x` sparisce; sul computer l'utente toglie `x` dalla playlist, e la
    /// rimozione arriva al telefono. Restituisce (computer, telefono).
    fn rimozione_vera_con_il_disco_staccato() -> (Connection, Connection) {
        let mut computer = libreria_con(&[]);
        let mut telefono = libreria_con(&[]);
        for connection in [&computer, &telefono] {
            for (id, chiave) in [(1, "a"), (2, "x"), (3, "b")] {
                brano_di_prova(connection, id, chiave, Some(&format!("c-{chiave}")));
            }
        }
        playlist_con(&computer, "sera", &[1, 2, 3]);
        allinea(&mut computer, "computer", 1_000).expect("allineamento");
        let del_computer = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", del_computer), 2_000);
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "x", "b"]);

        // Il disco del telefono si stacca: `x` parte (e il trigger ne ricorda il
        // posto).
        telefono
            .execute("DELETE FROM tracks WHERE track_key = 'x'", [])
            .expect("file sparito");

        // Sul computer l'utente toglie `x` dalla playlist, per davvero.
        computer
            .execute(
                "DELETE FROM playlist_tracks WHERE track_id = ?1",
                [id_del_brano(&computer, "x")],
            )
            .expect("tolto");
        allinea(&mut computer, "computer", 3_000).expect("allineamento");

        // Una passata: la rimozione arriva al telefono.
        let del_computer = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", del_computer), 4_000);
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);
        (computer, telefono)
    }

    #[test]
    fn una_rimozione_fatta_altrove_non_risorge_se_il_file_torna_dove_era_partito() {
        let (mut computer, mut telefono) = rimozione_vera_con_il_disco_staccato();

        // Il disco torna: la scansione reinserisce `x` e il trigger rimette il
        // posto da `playlist_orfane`. La sequenza, di `x`, ha solo lapidi.
        brano_di_prova(&telefono, 20, "x", Some("c-x"));
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "x", "b"]);
        allinea(&mut telefono, "telefono", 5_000).expect("allineamento");
        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "b"],
            "il ritorno di una voce tolta altrove non è un'aggiunta"
        );
        assert_eq!(
            sequenza_di(&telefono, "sera").ordine(),
            vec!["a", "b"],
            "e non è diventato una voce nuova"
        );

        let del_telefono = documento_di(&telefono, "telefono");
        let fuso = fondi(&[
            Documento::nuovo("computer", documento_di(&computer, "computer"), 6_000),
            Documento::nuovo("telefono", del_telefono, 6_000),
        ]);
        applica(&mut computer, &fuso, 6_000).expect("applicazione");
        assert_eq!(
            ordine(&computer, "sera"),
            vec!["a", "b"],
            "il brano che l'utente ha tolto sul computer non è risorto"
        );
    }

    #[test]
    fn dopo_il_ritorno_una_aggiunta_vera_del_brano_tolto_altrove_vale() {
        // Il rischio inverso ha una finestra sola: la passata che vede tornare
        // il file. Da quella dopo il brano non è più «tornato», e se l'utente lo
        // rimette nella playlist è una voce nuova come qualunque altra.
        let (_computer, mut telefono) = rimozione_vera_con_il_disco_staccato();
        brano_di_prova(&telefono, 20, "x", Some("c-x"));
        allinea(&mut telefono, "telefono", 5_000).expect("allineamento");
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);
        assert_eq!(
            settings::read(&telefono, CHIAVE_BRANI_ASSENTI),
            Ok(None),
            "il brano non manca più, e la memoria lo sa"
        );

        // L'utente lo rimette in fondo.
        telefono
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 SELECT id, 20, 2 FROM playlists WHERE playlist_key = 'sera'",
                [],
            )
            .expect("aggiunta a mano");
        allinea(&mut telefono, "telefono", 6_000).expect("allineamento");
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b", "x"]);
        assert_eq!(sequenza_di(&telefono, "sera").ordine(), vec!["a", "b", "x"]);
    }

    #[test]
    fn l_aggiunta_di_un_brano_appena_arrivato_a_una_playlist_che_non_lo_ha_mai_avuto_non_si_perde()
    {
        // La memoria è dei brani che mancavano, per tutte le playlist: `x` è
        // nella memoria perché «sera» lo nomina. L'utente lo mette anche in
        // «corsa», che non ha mai avuto né una voce né una lapide per lui, un
        // attimo dopo che il file è arrivato e prima della passata: è un gesto
        // vero, e non è il ritorno di una voce tolta.
        let (_computer, mut telefono) = computer_e_telefono();
        playlist_con(&telefono, "corsa", &[1]);
        allinea(&mut telefono, "telefono", 3_000).expect("allineamento");
        brano_di_prova(&telefono, 9, "x", None);
        telefono
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 SELECT id, 9, 1 FROM playlists WHERE playlist_key = 'corsa'",
                [],
            )
            .expect("aggiunta a mano");

        let fatto = allinea(&mut telefono, "telefono", 4_000).expect("allineamento");
        assert_eq!(ordine(&telefono, "corsa"), vec!["a", "x"], "{fatto:?}");
        assert_eq!(sequenza_di(&telefono, "corsa").ordine(), vec!["a", "x"]);
        // E in «sera», dove la voce c'era da prima, `x` rientra al suo posto.
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "x", "b"]);
    }

    struct Sim {
        nomi: [&'static str; 3],
        db: Vec<Connection>,
        pubblicato: Vec<Option<Contenuto>>,
        prossimo_id: Vec<i64>,
        adds: BTreeMap<(String, String), usize>,
        adesso: i64,
        log: Vec<String>,
        con_trigger: bool,
        seme: u64,
    }

    impl Sim {
        fn r(&mut self, n: usize) -> usize {
            self.seme ^= self.seme << 13;
            self.seme ^= self.seme >> 7;
            self.seme ^= self.seme << 17;
            usize::try_from(self.seme % u64::try_from(n.max(1)).unwrap_or(1)).unwrap_or(0)
        }

        fn nuovo(seme: u64, con_trigger: bool) -> Self {
            let chiavi: Vec<String> = (0..10).map(|i| format!("k{i}")).collect();
            let mut sim = Self {
                nomi: ["pc", "tel", "tab"],
                db: Vec::new(),
                pubblicato: vec![None, None, None],
                prossimo_id: vec![100, 100, 100],
                adds: BTreeMap::new(),
                adesso: 10_000,
                log: Vec::new(),
                con_trigger,
                seme: seme.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            };
            for d in 0..3 {
                let connection = db::open_in_memory()
                    .expect("database in memoria")
                    .connection;
                let mut quali: Vec<usize> = (0..10).collect();
                if d > 0 {
                    // sottoinsieme casuale da 3 a 6 brani
                    let quanti = 3 + sim.r(4);
                    for i in (1..quali.len()).rev() {
                        let j = sim.r(i + 1);
                        quali.swap(i, j);
                    }
                    quali.truncate(quanti);
                    quali.sort_unstable();
                }
                for i in quali {
                    let id = sim.prossimo_id[d];
                    sim.prossimo_id[d] += 1;
                    let ck = format!("c-{}-{}", sim.nomi[d], chiavi[i]);
                    // content_key uguale fra dispositivi per lo stesso brano: è del file
                    let ck = if con_trigger {
                        Some(format!("c-{}", chiavi[i]))
                    } else {
                        let _ = ck;
                        None
                    };
                    brano_di_prova(&connection, id, &chiavi[i], ck.as_deref());
                }
                sim.db.push(connection);
            }
            sim
        }

        fn tick(&mut self) -> i64 {
            self.adesso += 10;
            self.adesso
        }

        fn presenti(&self, d: usize) -> Vec<String> {
            let mut s = self.db[d]
                .prepare("SELECT track_key FROM tracks ORDER BY id")
                .expect("presenti");
            s.query_map([], |r| r.get::<_, String>(0))
                .expect("presenti")
                .map(|r| r.expect("riga"))
                .collect()
        }

        fn playlist_di(&self, d: usize) -> Vec<String> {
            let mut s = self.db[d]
                .prepare("SELECT playlist_key FROM playlists ORDER BY playlist_key")
                .expect("playlist");
            s.query_map([], |r| r.get::<_, String>(0))
                .expect("playlist")
                .map(|r| r.expect("riga"))
                .collect()
        }

        fn pt(&self, d: usize, playlist: &str) -> Vec<(i64, String)> {
            let mut s = self.db[d]
                .prepare(
                    "SELECT t.id, t.track_key FROM playlist_tracks pt
                       JOIN playlists p ON p.id = pt.playlist_id
                       JOIN tracks t ON t.id = pt.track_id
                      WHERE p.playlist_key = ?1 ORDER BY pt.position",
                )
                .expect("pt");
            s.query_map([playlist], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .expect("pt")
            .map(|r| r.expect("riga"))
            .collect()
        }

        fn scrivi_pt(&mut self, d: usize, playlist: &str, ids: &[i64]) {
            let t = self.tick();
            let pid = id_playlist(&self.db[d], playlist);
            self.db[d]
                .execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [pid])
                .expect("svuota");
            for (i, id) in ids.iter().enumerate() {
                self.db[d]
                    .execute(
                        "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                         VALUES (?1, ?2, ?3)",
                        rusqlite::params![pid, id, i64::try_from(i).expect("i")],
                    )
                    .expect("inserisce");
            }
            self.db[d]
                .execute(
                    "UPDATE playlists SET updated_at = ?2 WHERE id = ?1",
                    rusqlite::params![pid, t],
                )
                .expect("aggiorna");
        }

        fn sync(&mut self, d: usize) {
            let t = self.tick();
            let nome = self.nomi[d];
            allinea(&mut self.db[d], nome, t).expect("allineamento");
            let mio = documento_di(&self.db[d], nome);
            self.pubblicato[d] = Some(mio.clone());
            let documenti: Vec<Documento> = (0..3)
                .filter_map(|i| {
                    self.pubblicato[i]
                        .clone()
                        .map(|c| Documento::nuovo(self.nomi[i], c, t))
                })
                .collect();
            let fuso = fondi(&documenti);
            let esito = applica_se_ferma(&mut self.db[d], &mio, &fuso, t).expect("applicazione");
            assert!(esito.is_some(), "la libreria era ferma");
            self.log.push(format!("sync {nome}"));
        }

        fn istantanea(&self) -> Vec<String> {
            let mut v = Vec::new();
            for d in 0..3 {
                for p in self.playlist_di(d) {
                    let pt: Vec<String> = self.pt(d, &p).into_iter().map(|(_, k)| k).collect();
                    v.push(format!("{} {} pt={}", self.nomi[d], p, pt.join(",")));
                    let seq = sequenza_di(&self.db[d], &p);
                    let el: Vec<String> = seq
                        .elementi()
                        .map(|(i, e)| {
                            format!(
                                "{}:{}>{}{}",
                                i.testo(),
                                e.brano,
                                e.dopo.as_ref().map_or("-".to_owned(), Identita::testo),
                                if e.tolto.is_some() { "†" } else { "" }
                            )
                        })
                        .collect();
                    v.push(format!("{} {} seq={}", self.nomi[d], p, el.join(" ")));
                    v.push(format!("{} {} ordine={:?}", self.nomi[d], p, seq.ordine()));
                }
            }
            v
        }

        fn dettaglio(&self) -> String {
            if std::env::var("SIM_DETTAGLIO").is_ok() {
                format!(
                    "\nLOG {}\n{}",
                    self.log.join(" | "),
                    self.istantanea().join("\n")
                )
            } else {
                String::new()
            }
        }

        fn converge(&mut self) {
            for _ in 0..4 {
                for d in 0..3 {
                    self.sync(d);
                }
            }
            let prima = self.istantanea();
            for d in 0..3 {
                self.sync(d);
            }
            let dopo = self.istantanea();
            assert_eq!(prima, dopo, "non converge{}", self.dettaglio());
        }

        /// Il documento fuso di tutti, dopo la convergenza.
        fn fuso_finale(&mut self) -> Fuso {
            let t = self.tick();
            let mut docs = Vec::new();
            for d in 0..3 {
                docs.push(Documento::nuovo(
                    self.nomi[d],
                    documento_di(&self.db[d], self.nomi[d]),
                    t,
                ));
            }
            fondi(&docs)
        }

        fn op_aggiungi(&mut self, middle: bool) {
            let d = self.r(3);
            let playlists = self.playlist_di(d);
            if playlists.is_empty() {
                return;
            }
            let p = playlists[self.r(playlists.len())].clone();
            let presenti = self.presenti(d);
            if presenti.is_empty() {
                return;
            }
            let k = presenti[self.r(presenti.len())].clone();
            let id = id_del_brano(&self.db[d], &k);
            let mut ids: Vec<i64> = self.pt(d, &p).into_iter().map(|(i, _)| i).collect();
            let pos = if middle {
                self.r(ids.len() + 1)
            } else {
                ids.len()
            };
            ids.insert(pos, id);
            self.scrivi_pt(d, &p, &ids);
            *self.adds.entry((p.clone(), k.clone())).or_insert(0) += 1;
            self.log
                .push(format!("aggiungi {} {p} {k} @{pos}", self.nomi[d]));
        }

        fn op_sposta(&mut self) {
            let d = self.r(3);
            let playlists = self.playlist_di(d);
            if playlists.is_empty() {
                return;
            }
            let p = playlists[self.r(playlists.len())].clone();
            let mut ids: Vec<i64> = self.pt(d, &p).into_iter().map(|(i, _)| i).collect();
            if ids.len() < 2 {
                return;
            }
            let da = self.r(ids.len());
            let x = ids.remove(da);
            let a = self.r(ids.len() + 1);
            ids.insert(a, x);
            self.scrivi_pt(d, &p, &ids);
            self.log
                .push(format!("sposta {} {p} {da}->{a}", self.nomi[d]));
        }

        fn op_sparisce(&mut self) {
            let d = self.r(3);
            let presenti = self.presenti(d);
            if presenti.len() <= 2 {
                return;
            }
            let k = presenti[self.r(presenti.len())].clone();
            // Una passata ha visto quel che c'era prima che il file sparisse.
            let t0 = self.tick();
            let nome0 = self.nomi[d];
            allinea(&mut self.db[d], nome0, t0).expect("allineamento");
            self.db[d]
                .execute("DELETE FROM tracks WHERE track_key = ?1", [&k])
                .expect("sparisce");
            self.log.push(format!("sparisce {} {k}", self.nomi[d]));
            if !self.con_trigger {
                // Una passata che se ne accorge.
                let t = self.tick();
                let nome = self.nomi[d];
                allinea(&mut self.db[d], nome, t).expect("allineamento");
                self.log.push(format!("allinea-dopo-sparisce {nome}"));
            }
        }

        fn op_arriva(&mut self) {
            let d = self.r(3);
            let presenti = self.presenti(d);
            let mancanti: Vec<usize> = (0..10)
                .filter(|i| !presenti.contains(&format!("k{i}")))
                .collect();
            if mancanti.is_empty() {
                return;
            }
            let i = mancanti[self.r(mancanti.len())];
            let id = self.prossimo_id[d];
            self.prossimo_id[d] += 1;
            let ck = self.con_trigger.then(|| format!("c-k{i}"));
            brano_di_prova(&self.db[d], id, &format!("k{i}"), ck.as_deref());
            self.log.push(format!("arriva {} k{i}", self.nomi[d]));
        }

        fn controlla(&mut self, dopo_rimozioni: bool) {
            let fuso = self.fuso_finale();
            let adds = self.adds.clone();
            for (chiave_pl, pl) in &fuso.playlist {
                let mut vivi: BTreeMap<&str, usize> = BTreeMap::new();
                for brano in pl.sequenza.ordine() {
                    *vivi.entry(brano).or_insert(0) += 1;
                }
                for ((p, k), quante) in &adds {
                    if p != chiave_pl {
                        continue;
                    }
                    let v = vivi.get(k.as_str()).copied().unwrap_or(0);
                    if !dopo_rimozioni {
                        assert!(
                            v >= 1,
                            "PERDITA: {p}/{k} aggiunto {quante} volte, vivo {v}{}",
                            self.dettaglio()
                        );
                        assert!(
                            v <= *quante,
                            "DOPPIONE: {p}/{k} aggiunto {quante} volte, vivo {v}{}",
                            self.dettaglio()
                        );
                    }
                }
                // Coerenza dispositivo per dispositivo.
                for d in 0..3 {
                    let presenti = self.presenti(d);
                    let mut attesi: BTreeMap<String, usize> = BTreeMap::new();
                    for (k, n) in &vivi {
                        if presenti.iter().any(|p| p == k) {
                            attesi.insert((*k).to_owned(), *n);
                        }
                    }
                    let mut visti: BTreeMap<String, usize> = BTreeMap::new();
                    for (_, k) in self.pt(d, chiave_pl) {
                        *visti.entry(k).or_insert(0) += 1;
                    }
                    assert_eq!(
                        visti,
                        attesi,
                        "INCOERENTE su {} per {chiave_pl}{}",
                        self.nomi[d],
                        self.dettaglio()
                    );
                    // E anche l'ordine: quello della sequenza, tolti i brani che qui non ci sono.
                    let in_ordine_atteso: Vec<String> = pl
                        .sequenza
                        .ordine()
                        .into_iter()
                        .filter(|b| presenti.iter().any(|p| p == b))
                        .map(str::to_owned)
                        .collect();
                    let in_ordine_visto: Vec<String> =
                        self.pt(d, chiave_pl).into_iter().map(|(_, k)| k).collect();
                    assert_eq!(
                        in_ordine_visto,
                        in_ordine_atteso,
                        "ORDINE su {} per {chiave_pl}{}",
                        self.nomi[d],
                        self.dettaglio()
                    );
                }
            }
        }
    }

    fn una_simulazione(seme: u64, con_trigger: bool, con_arrivi_e_partenze: bool) {
        let mut sim = Sim::nuovo(seme, con_trigger);
        // Il computer crea due playlist.
        for p in ["P", "Q"] {
            let presenti = sim.presenti(0);
            let quanti = 3 + sim.r(4);
            let mut ids = Vec::new();
            let mut chiavi = Vec::new();
            for _ in 0..quanti {
                let k = presenti[sim.r(presenti.len())].clone();
                ids.push(id_del_brano(&sim.db[0], &k));
                chiavi.push(k);
            }
            sim.db[0]
                .execute(
                    "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                     VALUES (?1, ?1, 100, 100)",
                    [p],
                )
                .expect("playlist");
            sim.scrivi_pt(0, p, &ids);
            for k in chiavi {
                *sim.adds.entry((p.to_owned(), k)).or_insert(0) += 1;
            }
        }
        sim.converge();
        let passi = 20 + sim.r(25);
        for _ in 0..passi {
            match sim.r(10) {
                0 | 1 => sim.op_aggiungi(false),
                2 => {
                    // un riordino per volta, a libreria ferma
                    sim.converge();
                    sim.op_sposta();
                    sim.converge();
                }
                9 => {
                    // un inserimento a metà, a libreria ferma
                    sim.converge();
                    sim.op_aggiungi(true);
                    sim.converge();
                }
                3 if con_arrivi_e_partenze => sim.op_sparisce(),
                4 | 5 if con_arrivi_e_partenze => sim.op_arriva(),
                _ => {
                    let d = sim.r(3);
                    sim.sync(d);
                }
            }
        }
        sim.converge();
        sim.controlla(false);
    }

    /// Quanti semi provare: pochi nella suite di tutti i giorni, tanti con
    /// `SIM_SEMI=150 cargo test -p rymembra-app --lib -- sincronia::prove::simulazione`
    /// (il ciclo completo, quello della revisione, dura qualche minuto in debug).
    fn quanti_semi(di_serie: u64) -> u64 {
        std::env::var("SIM_SEMI")
            .ok()
            .and_then(|valore| valore.parse().ok())
            .unwrap_or(di_serie)
    }

    fn tutti(con_trigger: bool, arrivi: bool, di_serie: u64) {
        for seme in 0..quanti_semi(di_serie) {
            let esito = std::panic::catch_unwind(|| una_simulazione(seme, con_trigger, arrivi));
            if let Err(e) = esito {
                eprintln!("SEME {seme} (trigger {con_trigger}, arrivi {arrivi})");
                std::panic::resume_unwind(e);
            }
        }
    }

    #[test]
    fn simulazione_senza_arrivi_ne_partenze() {
        tutti(false, false, 10);
    }

    #[test]
    fn simulazione_con_arrivi_e_partenze_senza_trigger() {
        tutti(false, true, 15);
    }

    #[test]
    fn simulazione_con_arrivi_e_partenze_con_trigger() {
        tutti(true, true, 15);
    }

    #[test]
    fn un_seme_della_simulazione() {
        let Ok(seme) = std::env::var("SIM_SEME") else {
            return;
        };
        let seme: u64 = seme.parse().expect("seme");
        let trigger = std::env::var("SIM_TRIGGER").is_ok_and(|v| v == "1");
        let arrivi = std::env::var("SIM_ARRIVI").is_ok_and(|v| v == "1");
        una_simulazione(seme, trigger, arrivi);
    }

    /// Dopo la convergenza: toglie una riga da una playlist su un dispositivo, converge, poi fa
    /// arrivare il brano dove manca: la rimozione deve restare.
    fn rimozione_e_arrivi_tardivi(seme: u64, con_trigger: bool) {
        let mut sim = Sim::nuovo(seme, con_trigger);
        for p in ["P", "Q"] {
            let presenti = sim.presenti(0);
            let quanti = 3 + sim.r(4);
            let mut ids = Vec::new();
            let mut chiavi = Vec::new();
            for _ in 0..quanti {
                let k = presenti[sim.r(presenti.len())].clone();
                if chiavi.contains(&k) {
                    continue;
                }
                ids.push(id_del_brano(&sim.db[0], &k));
                chiavi.push(k);
            }
            sim.db[0]
                .execute(
                    "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                     VALUES (?1, ?1, 100, 100)",
                    [p],
                )
                .expect("playlist");
            sim.scrivi_pt(0, p, &ids);
        }
        sim.converge();
        // Qualche partenza e arrivo prima, per avere memoria e orfani in giro.
        for _ in 0..(2 + sim.r(6)) {
            match sim.r(4) {
                0 => sim.op_sparisce(),
                1 => sim.op_arriva(),
                _ => {
                    let d = sim.r(3);
                    sim.sync(d);
                }
            }
        }
        sim.converge();
        // Un gesto di rimozione su un dispositivo, di una riga che c'è.
        let mut fatto = None;
        for _ in 0..20 {
            let d = sim.r(3);
            let playlists = sim.playlist_di(d);
            let p = playlists[sim.r(playlists.len())].clone();
            let righe = sim.pt(d, &p);
            if righe.is_empty() {
                continue;
            }
            let i = sim.r(righe.len());
            let k = righe[i].1.clone();
            let mut ids: Vec<i64> = righe.iter().map(|(id, _)| *id).collect();
            ids.remove(i);
            sim.scrivi_pt(d, &p, &ids);
            sim.log.push(format!("RIMUOVE {} {p} {k}", sim.nomi[d]));
            fatto = Some((p, k));
            break;
        }
        let Some((p, k)) = fatto else {
            return;
        };
        sim.converge();
        // Dove il brano manca, arriva adesso.
        for d in 0..3 {
            if !sim.presenti(d).contains(&k) {
                let id = sim.prossimo_id[d];
                sim.prossimo_id[d] += 1;
                let ck = sim.con_trigger.then(|| format!("c-{k}"));
                brano_di_prova(&sim.db[d], id, &k, ck.as_deref());
                sim.log.push(format!("arriva-dopo {} {k}", sim.nomi[d]));
            }
        }
        sim.converge();
        // Il brano non deve essere tornato in `p` in più di quanti ne restavano.
        let fuso = sim.fuso_finale();
        let vivi = fuso.playlist[&p]
            .sequenza
            .ordine()
            .into_iter()
            .filter(|b| *b == k)
            .count();
        for d in 0..3 {
            let qui = sim.pt(d, &p).into_iter().filter(|(_, b)| *b == k).count();
            assert_eq!(
                qui,
                vivi,
                "INCOERENTE dopo la rimozione su {} per {p}/{k}{}",
                sim.nomi[d],
                sim.dettaglio()
            );
        }
        // Prima della rimozione `k` stava in `p` una volta sola (le playlist
        // iniziali non hanno doppioni e non si aggiungono brani): ora zero.
        assert_eq!(
            vivi,
            0,
            "RISORTO: {p}/{k} è tornato dopo una rimozione{}",
            sim.dettaglio()
        );
    }

    fn tutti_rimozione(con_trigger: bool, di_serie: u64) {
        for seme in 0..quanti_semi(di_serie) {
            let esito = std::panic::catch_unwind(|| rimozione_e_arrivi_tardivi(seme, con_trigger));
            if let Err(e) = esito {
                eprintln!("SEME {seme} (rimozione, trigger {con_trigger})");
                std::panic::resume_unwind(e);
            }
        }
    }

    #[test]
    fn simulazione_di_rimozione_senza_trigger() {
        tutti_rimozione(false, 15);
    }

    /// Il seme 8 era quello rosso: una rimozione vera risorgeva quando il file
    /// tornava e il trigger rimetteva il posto.
    #[test]
    fn simulazione_di_rimozione_con_trigger() {
        tutti_rimozione(true, 15);
    }

    #[test]
    fn un_seme_della_simulazione_di_rimozione() {
        let Ok(seme) = std::env::var("SIM_SEME_RIMOZIONE") else {
            return;
        };
        let seme: u64 = seme.parse().expect("seme");
        let trigger = std::env::var("SIM_TRIGGER").is_ok_and(|v| v == "1");
        rimozione_e_arrivi_tardivi(seme, trigger);
    }

    // ── Dalla seconda revisione indipendente della sincronia (FSYNC2): cancella e rifai fra
    // dispositivi, «Annulla», rinomine incrociate, e tre simulazioni a tre dispositivi. ──

    fn ora_vera() -> i64 {
        crate::library::now_ms()
    }

    // ---- #8: Annulla dopo una passata, con un brano invisibile che altrove è vivo ----
    #[test]
    fn annulla_dopo_una_passata_rida_al_computer_il_brano_che_il_telefono_non_ha() {
        let (mut computer, mut telefono) = computer_e_telefono();
        let base = ora_vera();
        let id = id_playlist(&telefono, "sera");
        let cancellata = crate::playlists::cancella(&mut telefono, id).expect("cancella");

        // Una passata fra la cancellazione e l'«Annulla».
        let del_computer = documento_di(&computer, "computer");
        passata_con_altro(
            &mut telefono,
            "telefono",
            ("computer", del_computer),
            base + 10,
        );
        let del_telefono = documento_di(&telefono, "telefono");
        let fuso = fondi(&[
            Documento::nuovo("computer", documento_di(&computer, "computer"), base + 20),
            Documento::nuovo("telefono", del_telefono, base + 20),
        ]);
        applica(&mut computer, &fuso, base + 20).expect("applicazione");
        assert!(
            ordine(&computer, "sera").is_empty(),
            "la cancellazione è arrivata"
        );

        std::thread::sleep(std::time::Duration::from_millis(3));
        crate::playlists::ripristina(&mut telefono, &cancellata).expect("ripristina");
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);

        // Un giro completo dopo l'«Annulla».
        for t in 0..4 {
            let del_computer = documento_di(&computer, "computer");
            passata_con_altro(
                &mut telefono,
                "telefono",
                ("computer", del_computer),
                base + 100 + t * 10,
            );
            allinea(&mut computer, "computer", base + 105 + t * 10).expect("allinea");
            let del_telefono = documento_di(&telefono, "telefono");
            let fuso = fondi(&[
                Documento::nuovo(
                    "computer",
                    documento_di(&computer, "computer"),
                    base + 106 + t * 10,
                ),
                Documento::nuovo("telefono", del_telefono, base + 106 + t * 10),
            ]);
            applica(&mut computer, &fuso, base + 106 + t * 10).expect("applicazione");
        }
        assert_eq!(
            ordine(&computer, "sera"),
            vec!["a", "x", "b"],
            "dopo «Annulla» il computer ha di nuovo tutta la playlist, x compreso"
        );
        assert_eq!(sequenza_di(&telefono, "sera").ordine(), vec!["a", "x", "b"]);
    }

    // ---- #1: rinomina incrociata fra due dispositivi ----
    #[test]
    fn rinomine_diverse_su_due_dispositivi_non_perdono_x() {
        let (mut computer, mut telefono) = computer_e_telefono();
        let base = ora_vera();
        let id_t = id_playlist(&telefono, "sera");
        crate::playlists::rename(&telefono, id_t, "notte").expect("rinomina");
        let id_c = id_playlist(&computer, "sera");
        crate::playlists::rename(&computer, id_c, "alba").expect("rinomina");
        for t in 0..5 {
            let tt = base + 100 + t * 50;
            allinea(&mut computer, "computer", tt).expect("allinea");
            let dc = documento_di(&computer, "computer");
            passata_con_altro(&mut telefono, "telefono", ("computer", dc), tt + 10);
            let dt = documento_di(&telefono, "telefono");
            let fuso = fondi(&[
                Documento::nuovo("computer", documento_di(&computer, "computer"), tt + 20),
                Documento::nuovo("telefono", dt, tt + 20),
            ]);
            applica(&mut computer, &fuso, tt + 20).expect("applicazione");
        }
        let mut nomi_c: Vec<String> = computer
            .prepare("SELECT playlist_key FROM playlists ORDER BY 1")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        nomi_c.sort();
        eprintln!("computer: {nomi_c:?}");
        for k in &nomi_c {
            assert_eq!(ordine(&computer, k), vec!["a", "x", "b"], "computer {k}");
            assert_eq!(ordine(&telefono, k), vec!["a", "b"], "telefono {k}");
        }
        assert!(!nomi_c.is_empty());
    }

    // ---- #1: rinomina e subito di nuovo il nome di prima, con una passata nel mezzo ----
    #[test]
    fn rinomina_avanti_e_indietro_con_una_passata_nel_mezzo() {
        let (mut computer, mut telefono) = computer_e_telefono();
        let base = ora_vera();
        let id_t = id_playlist(&telefono, "sera");
        crate::playlists::rename(&telefono, id_t, "notte").expect("rinomina");
        let dc = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", dc), base + 10);
        let dt = documento_di(&telefono, "telefono");
        let fuso = fondi(&[
            Documento::nuovo("computer", documento_di(&computer, "computer"), base + 20),
            Documento::nuovo("telefono", dt, base + 20),
        ]);
        applica(&mut computer, &fuso, base + 20).expect("applicazione");
        std::thread::sleep(std::time::Duration::from_millis(3));
        let id_t = id_playlist(&telefono, "notte");
        crate::playlists::rename(&telefono, id_t, "sera").expect("rinomina indietro");
        for t in 0..5 {
            let tt = base + 100 + t * 50;
            allinea(&mut computer, "computer", tt).expect("allinea");
            let dc = documento_di(&computer, "computer");
            passata_con_altro(&mut telefono, "telefono", ("computer", dc), tt + 10);
            let dt = documento_di(&telefono, "telefono");
            let fuso = fondi(&[
                Documento::nuovo("computer", documento_di(&computer, "computer"), tt + 20),
                Documento::nuovo("telefono", dt, tt + 20),
            ]);
            applica(&mut computer, &fuso, tt + 20).expect("applicazione");
        }
        let nomi_c: Vec<String> = computer
            .prepare("SELECT playlist_key FROM playlists ORDER BY 1")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        eprintln!("computer: {nomi_c:?}");
        for k in &nomi_c {
            assert_eq!(ordine(&computer, k), vec!["a", "x", "b"], "computer {k}");
        }
        assert_eq!(ordine(&computer, "sera"), vec!["a", "x", "b"]);
    }

    // ---- simulazioni estese: rimozioni e riaggiunte miste ad arrivi e partenze ----
    impl Sim {
        fn op_togli_riga(&mut self, toccati: &mut BTreeSet<(String, String)>) {
            let d = self.r(3);
            let playlists = self.playlist_di(d);
            if playlists.is_empty() {
                return;
            }
            let p = playlists[self.r(playlists.len())].clone();
            let righe = self.pt(d, &p);
            if righe.is_empty() {
                return;
            }
            let i = self.r(righe.len());
            let k = righe[i].1.clone();
            let mut ids: Vec<i64> = righe.iter().map(|(id, _)| *id).collect();
            ids.remove(i);
            self.scrivi_pt(d, &p, &ids);
            toccati.insert((p.clone(), k.clone()));
            self.log.push(format!("RIMUOVE {} {p} {k}", self.nomi[d]));
        }

        /// Coerenza fra i dispositivi: stessi elementi vivi e stesso ordine.
        fn coerenza(&mut self) {
            let fuso = self.fuso_finale();
            for (chiave_pl, pl) in &fuso.playlist {
                for d in 0..3 {
                    let presenti = self.presenti(d);
                    let atteso: Vec<String> = pl
                        .sequenza
                        .ordine()
                        .into_iter()
                        .filter(|b| presenti.iter().any(|p| p == b))
                        .map(str::to_owned)
                        .collect();
                    let visto: Vec<String> =
                        self.pt(d, chiave_pl).into_iter().map(|(_, k)| k).collect();
                    assert_eq!(
                        visto,
                        atteso,
                        "INCOERENTE su {} per {chiave_pl}{}",
                        self.nomi[d],
                        self.dettaglio()
                    );
                }
            }
        }
    }

    fn crea_le_due(sim: &mut Sim) {
        for p in ["P", "Q"] {
            let presenti = sim.presenti(0);
            let quanti = 3 + sim.r(4);
            let mut ids = Vec::new();
            let mut chiavi = Vec::new();
            for _ in 0..quanti {
                let k = presenti[sim.r(presenti.len())].clone();
                ids.push(id_del_brano(&sim.db[0], &k));
                chiavi.push(k);
            }
            sim.db[0]
                .execute(
                    "INSERT INTO playlists (playlist_key, name, created_at, updated_at)
                     VALUES (?1, ?1, 100, 100)",
                    [p],
                )
                .expect("playlist");
            sim.scrivi_pt(0, p, &ids);
            for k in chiavi {
                *sim.adds.entry((p.to_owned(), k)).or_insert(0) += 1;
            }
        }
    }

    fn sim_mista(seme: u64, con_trigger: bool) {
        let mut sim = Sim::nuovo(seme, con_trigger);
        crea_le_due(&mut sim);
        sim.converge();
        let mut toccati: BTreeSet<(String, String)> = BTreeSet::new();
        let passi = 25 + sim.r(30);
        for _ in 0..passi {
            match sim.r(12) {
                0 | 1 => sim.op_aggiungi(false),
                2 => {
                    sim.converge();
                    sim.op_sposta();
                    sim.converge();
                }
                3 => sim.op_sparisce(),
                4 | 5 => sim.op_arriva(),
                6 => sim.op_togli_riga(&mut toccati),
                7 => {
                    sim.converge();
                    sim.op_aggiungi(true);
                    sim.converge();
                }
                _ => {
                    let d = sim.r(3);
                    sim.sync(d);
                }
            }
        }
        sim.converge();
        sim.coerenza();
        // Niente perso fra quel che nessuno ha toccato.
        let fuso = sim.fuso_finale();
        for ((p, k), quante) in sim.adds.clone() {
            if toccati.contains(&(p.clone(), k.clone())) {
                continue;
            }
            let Some(pl) = fuso.playlist.get(&p) else {
                continue;
            };
            let v = pl.sequenza.ordine().into_iter().filter(|b| *b == k).count();
            assert!(
                v >= 1,
                "PERDITA {p}/{k} (aggiunto {quante}){}",
                sim.dettaglio()
            );
        }
    }

    #[test]
    fn simulazione_mista_a_tre_dispositivi() {
        for trigger in [false, true] {
            for seme in 0..quanti_semi(40) {
                let esito = std::panic::catch_unwind(|| sim_mista(seme, trigger));
                if let Err(e) = esito {
                    eprintln!("SEME {seme} (mista, trigger {trigger})");
                    std::panic::resume_unwind(e);
                }
            }
        }
    }

    #[test]
    fn un_seme_sim_mista() {
        let Ok(seme) = std::env::var("REV2_SEME") else {
            return;
        };
        let trigger = std::env::var("SIM_TRIGGER").is_ok_and(|v| v == "1");
        sim_mista(seme.parse().expect("seme"), trigger);
    }

    // ---- simulazione: rinomine e cancellazioni con «Annulla», sempre a libreria ferma ----
    impl Sim {
        fn chiave_di(&self, d: usize, id: i64) -> String {
            self.db[d]
                .query_row(
                    "SELECT playlist_key FROM playlists WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .expect("chiave")
        }

        fn op_rinomina(&mut self) {
            self.converge();
            let d = self.r(3);
            let playlists = self.playlist_di(d);
            if playlists.is_empty() {
                return;
            }
            let p = playlists[self.r(playlists.len())].clone();
            let nuovo = format!("{p}x");
            let id = id_playlist(&self.db[d], &p);
            if crate::playlists::rename(&self.db[d], id, &nuovo).is_err() {
                return;
            }
            let nuova_chiave = self.chiave_di(d, id);
            let voci: Vec<((String, String), usize)> = self
                .adds
                .iter()
                .filter(|((pp, _), _)| *pp == p)
                .map(|(k, n)| (k.clone(), *n))
                .collect();
            for ((_, k), n) in voci {
                self.adds.remove(&(p.clone(), k.clone()));
                self.adds.insert((nuova_chiave.clone(), k), n);
            }
            self.log
                .push(format!("RINOMINA {} {p} -> {nuova_chiave}", self.nomi[d]));
            self.converge();
        }

        fn op_cancella_annulla(&mut self, con_passata: bool, converge_prima: bool) {
            if converge_prima {
                self.converge();
            }
            let d = self.r(3);
            let playlists = self.playlist_di(d);
            if playlists.is_empty() {
                return;
            }
            let p = playlists[self.r(playlists.len())].clone();
            let id = id_playlist(&self.db[d], &p);
            let c = crate::playlists::cancella(&mut self.db[d], id).expect("cancella");
            self.log.push(format!(
                "CANCELLA {} {p} (passata {con_passata})",
                self.nomi[d]
            ));
            if con_passata {
                for e in 0..3 {
                    self.sync(e);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(3));
            crate::playlists::ripristina(&mut self.db[d], &c).expect("ripristina");
            self.log.push(format!("ANNULLA {} {p}", self.nomi[d]));
            self.converge();
        }
    }

    fn sim_rinomina(seme: u64, con_trigger: bool) {
        let mut sim = Sim::nuovo(seme, con_trigger);
        crea_le_due(&mut sim);
        sim.converge();
        let passi = 15 + sim.r(20);
        for _ in 0..passi {
            match sim.r(12) {
                0 | 1 => sim.op_aggiungi(false),
                2 => {
                    sim.converge();
                    sim.op_sposta();
                    sim.converge();
                }
                3 => sim.op_sparisce(),
                4 | 5 => sim.op_arriva(),
                6 => sim.op_rinomina(),
                7 => {
                    let con_passata = sim.r(2) == 0;
                    sim.op_cancella_annulla(con_passata, true);
                }
                _ => {
                    let d = sim.r(3);
                    sim.sync(d);
                }
            }
        }
        sim.converge();
        sim.controlla(false);
    }

    #[test]
    fn simulazione_rinomina_e_annulla() {
        for trigger in [false, true] {
            for seme in 0..quanti_semi(40) {
                let esito = std::panic::catch_unwind(|| sim_rinomina(seme, trigger));
                if let Err(e) = esito {
                    eprintln!("SEME {seme} (rinomina, trigger {trigger})");
                    std::panic::resume_unwind(e);
                }
            }
        }
    }

    #[test]
    fn un_seme_sim_rinomina() {
        let Ok(seme) = std::env::var("REV2_SEME") else {
            return;
        };
        let trigger = std::env::var("SIM_TRIGGER").is_ok_and(|v| v == "1");
        sim_rinomina(seme.parse().expect("seme"), trigger);
    }

    // ---- a regime, `allinea` non scrive e non riaggancia niente ----
    fn sim_idempotenza(seme: u64, con_trigger: bool) {
        let mut sim = Sim::nuovo(seme, con_trigger);
        crea_le_due(&mut sim);
        sim.converge();
        let mut toccati: BTreeSet<(String, String)> = BTreeSet::new();
        let passi = 25 + sim.r(30);
        for _ in 0..passi {
            match sim.r(12) {
                0 | 1 => sim.op_aggiungi(false),
                3 => sim.op_sparisce(),
                4 | 5 => sim.op_arriva(),
                6 => sim.op_togli_riga(&mut toccati),
                _ => {
                    let d = sim.r(3);
                    sim.sync(d);
                }
            }
        }
        sim.converge();
        for d in 0..3 {
            let t = sim.tick();
            let nome = sim.nomi[d];
            let fatto = allinea(&mut sim.db[d], nome, t).expect("allinea");
            assert!(
                fatto.e_vuoto(),
                "NON A REGIME su {nome}: {fatto:?}{}",
                sim.dettaglio()
            );
        }
    }

    #[test]
    fn simulazione_allinea_a_regime_non_scrive() {
        for trigger in [false, true] {
            for seme in 0..quanti_semi(40) {
                let esito = std::panic::catch_unwind(|| sim_idempotenza(seme, trigger));
                if let Err(e) = esito {
                    eprintln!("SEME {seme} (idempotenza, trigger {trigger})");
                    std::panic::resume_unwind(e);
                }
            }
        }
    }

    // ---- playlist cancellata su un dispositivo e rifatta con lo stesso nome su un altro ----
    #[test]
    fn cancellata_sul_computer_e_rifatta_sul_telefono_con_un_brano_solo_suo() {
        // Il computer ha a, x, b; il telefono a, b, y (y solo suo).
        let mut computer = libreria_con(&["a", "x", "b"]);
        let mut telefono = libreria_con(&["a", "b", "y"]);
        let base = ora_vera();
        playlist_con(&computer, "sera", &[1, 2, 3]);
        let giro = |computer: &mut Connection, telefono: &mut Connection, da: i64, volte: i64| {
            for t in 0..volte {
                let tt = da + t * 100;
                allinea(computer, "computer", tt).expect("allinea");
                let dc = documento_di(computer, "computer");
                passata_con_altro(telefono, "telefono", ("computer", dc), tt + 10);
                let dt = documento_di(telefono, "telefono");
                let fuso = fondi(&[
                    Documento::nuovo("computer", documento_di(computer, "computer"), tt + 20),
                    Documento::nuovo("telefono", dt, tt + 20),
                ]);
                applica(computer, &fuso, tt + 20).expect("applicazione");
            }
        };
        giro(&mut computer, &mut telefono, base + 100, 3);
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "b"]);

        // Il computer cancella «sera»; la cancellazione arriva al telefono.
        let id = id_playlist(&computer, "sera");
        crate::playlists::cancella(&mut computer, id).expect("cancella");
        giro(&mut computer, &mut telefono, base + 1_000, 3);
        assert!(
            ordine(&telefono, "sera").is_empty(),
            "la cancellazione è arrivata"
        );

        // Il telefono rifà una «sera» sua, con un brano solo suo.
        std::thread::sleep(std::time::Duration::from_millis(3));
        playlist_con(&telefono, "sera", &[1, 3, 2]); // a, y, b? ids: telefono a=1 b=2 y=3
        let adesso = ora_vera();
        telefono
            .execute(
                "UPDATE playlists SET created_at = ?1, updated_at = ?1 WHERE playlist_key = 'sera'",
                [adesso],
            )
            .expect("data");
        assert_eq!(ordine(&telefono, "sera"), vec!["a", "y", "b"]);
        giro(&mut computer, &mut telefono, base + 10_000, 5);
        eprintln!(
            "telefono: {:?}  computer: {:?}",
            ordine(&telefono, "sera"),
            ordine(&computer, "sera")
        );
        for (i, e) in sequenza_di(&telefono, "sera").elementi() {
            eprintln!(
                "  telefono seq {} {} tolto={:?}",
                i.testo(),
                e.brano,
                e.tolto.is_some()
            );
        }
        assert_eq!(
            ordine(&telefono, "sera"),
            vec!["a", "y", "b"],
            "il telefono perde un brano che aveva messo lui"
        );
    }

    // ---- #8 (+ radice di #1): playlist cancellata dal telefono, e il computer la tocca DOPO la lapide ----
    #[test]
    fn cancellata_dal_telefono_e_ritoccata_dal_computer_dopo_la_lapide_x_resta() {
        let (mut computer, mut telefono) = computer_e_telefono();
        let base = crate::library::now_ms();
        let id: i64 = telefono
            .query_row(
                "SELECT id FROM playlists WHERE playlist_key = 'sera'",
                [],
                |r| r.get(0),
            )
            .expect("id");
        crate::playlists::cancella(&mut telefono, id).expect("cancella");
        // Una passata del telefono: la lapide parte. Non si applica ancora al computer.
        let dc = documento_di(&computer, "computer");
        passata_con_altro(&mut telefono, "telefono", ("computer", dc), base + 10);

        // Il computer, scollegato, aggiunge `z` a «sera» DOPO la lapide.
        brano_di_prova(&computer, 4, "z", None);
        computer
            .execute(
                "INSERT INTO playlist_tracks (playlist_id, track_id, position)
                 SELECT id, 4, 3 FROM playlists WHERE playlist_key = 'sera'",
                [],
            )
            .expect("aggiunta");
        computer
            .execute(
                "UPDATE playlists SET updated_at = ?1 WHERE playlist_key = 'sera'",
                [base + 500],
            )
            .expect("data");

        for t in 0..6 {
            let tt = base + 1_000 + t * 100;
            allinea(&mut computer, "computer", tt).expect("allinea");
            let dc = documento_di(&computer, "computer");
            passata_con_altro(&mut telefono, "telefono", ("computer", dc), tt + 10);
            let dt = documento_di(&telefono, "telefono");
            let fuso = fondi(&[
                Documento::nuovo("computer", documento_di(&computer, "computer"), tt + 20),
                Documento::nuovo("telefono", dt, tt + 20),
            ]);
            applica(&mut computer, &fuso, tt + 20).expect("applicazione");
        }
        eprintln!("computer: {:?}", ordine(&computer, "sera"));
        eprintln!("telefono: {:?}", ordine(&telefono, "sera"));
        let seq = sequenza_di(&computer, "sera");
        for (i, e) in seq.elementi() {
            eprintln!(
                "  computer seq {} {} dopo={:?} tolto={:?}",
                i.testo(),
                e.brano,
                e.dopo.as_ref().map(Identita::testo),
                e.tolto.is_some()
            );
        }
        assert!(
            ordine(&computer, "sera").contains(&"x".to_owned()),
            "x (il brano che il telefono non ha) è sparito dalla playlist del computer"
        );
    }

    /// Misura (ignorata): la query dei brani assenti con tante lapidi.
    #[test]
    #[ignore = "misura"]
    fn misura_brani_assenti_con_le_lapidi() {
        use std::time::Instant;
        let telefono = db::open_in_memory().expect("db").connection;
        {
            let tx = telefono.unchecked_transaction().expect("tx");
            for i in 0..2_500_i64 {
                let chiave = format!("artista {i}|titolo {i}|album dove");
                tx.execute(
                    "INSERT INTO tracks (id, path, track_key, title, artist, album, file_size, date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?3, 'chi', 'dove', 0, 0, 0)",
                    rusqlite::params![i + 1, format!("/musica/{i}.flac"), chiave],
                ).expect("brano");
            }
            // 200 playlist, 100 vive e 900 lapidi ciascuna: 200 000 righe.
            let mut seme: u64 = 0x2545_F491_4F6C_DD1D;
            for p in 0..200 {
                for e in 0..1000 {
                    seme ^= seme << 13;
                    seme ^= seme >> 7;
                    seme ^= seme << 17;
                    let k = seme % 50_000;
                    let tolto: Option<i64> = if e < 100 { None } else { Some(5) };
                    tx.execute(
                        "INSERT INTO sync_sequenza (playlist_key, elemento, dopo, track_key, tolto_ms) VALUES (?1, ?2, NULL, ?3, ?4)",
                        rusqlite::params![format!("p{p}"), format!("d:{e}"), format!("artista {k}|titolo {k}|album dove"), tolto],
                    ).expect("riga");
                }
            }
            tx.commit().expect("commit");
        }
        let t0 = Instant::now();
        let tutte = brani_assenti(&telefono).expect("assenti").senza_riga;
        eprintln!(
            "MISURA nuova (tutte le righe): {:?}, {} chiavi",
            t0.elapsed(),
            tutte.len()
        );
        let t0 = Instant::now();
        let mut st = telefono
            .prepare(
                "SELECT DISTINCT s.track_key FROM sync_sequenza s WHERE s.tolto_ms IS NULL
              AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.track_key = s.track_key)",
            )
            .expect("p");
        let vive: BTreeSet<String> = st
            .query_map([], |r| r.get::<_, String>(0))
            .expect("q")
            .map(|r| r.expect("r"))
            .collect();
        eprintln!(
            "MISURA vecchia (solo vive): {:?}, {} chiavi",
            t0.elapsed(),
            vive.len()
        );
        let json = serde_json::to_string(&tutte).expect("json");
        eprintln!(
            "MISURA memoria: {} byte di JSON (solo vive: {} byte)",
            json.len(),
            serde_json::to_string(&vive).expect("json").len()
        );
    }
}
