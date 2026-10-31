//! La camminata sul disco, radice per radice e col silenzio misurato, e le cartelle da
//! riguardare quando la sorveglianza dice che qualcosa è cambiato.

use super::*;

/// Com'è andata la camminata su **una** radice.
///
/// I tre casi sono tre reazioni diverse, ed è per questo che sono un tipo e non
/// un `Result<Camminata, _>`: visto si giudica, arenato si salta, fermato si
/// smette del tutto.
enum Passeggiata {
    /// La camminata è finita e ha consegnato quel che ha trovato.
    Vista {
        /// I file consegnati.
        file: Vec<DiscoveredFile>,
        /// Nessun ramo perso: vedi [`crate::files::Camminata::completa`].
        completa: bool,
        /// I rami persi, per nome: vedi [`Voce::RamoPerso`].
        rami: Vec<String>,
    },
    /// La camminata non è finita: da troppo non arriva più niente.
    ///
    /// Non è «vuota» e non è «parziale», è **non tornata**. Su una condivisione
    /// che non risponde è la forma che prende il guasto, perché il filesystem
    /// non dà nessun errore: resta lì.
    Arenata,
    /// Qualcuno ha chiesto di smettere.
    Fermata,
}

/// Quel che la camminata consegna, un pezzo per volta.
enum Passo {
    /// Un file.
    Trovato(DiscoveredFile),
    /// Una cartella dentro cui non si è potuto guardare.
    RamoPerso(String),
    /// Una cartella elencata: nessun file, ma il disco ha risposto.
    Cartella,
    /// Non ce ne sono altri.
    Fine {
        /// Nessun ramo perso.
        completa: bool,
    },
    /// La camminata è fallita.
    ///
    /// L'errore viaggia col passo e non lo legge nessuno, ed è una scelta: chi
    /// consuma reagisce a qualunque guasto allo stesso modo — salta la radice —
    /// e non ha dove scriverlo, perché [`ScanReport::unreadable`] è un elenco di
    /// **file** e questo è un guasto che riguarda una cartella intera. Resta
    /// attaccato al passo perché è la sola cosa che dica *perché* quella radice
    /// è uscita dall'esito, e il giorno in cui la si vorrà mettere nel diario
    /// dev'essere già lì invece che ricostruita da capo.
    #[expect(
        dead_code,
        reason = "la causa se la porta appresso il passo che l'ha prodotta; chi salta la radice non ha dove scriverla"
    )]
    Caduta(AppError),
}

/// Cammina una radice sola, smettendo di aspettare se smette di arrivare roba.
///
/// La camminata gira su un filo suo e consegna un [`Passo`] per file; qui si
/// consuma con un `recv_timeout` da un [`BATTITO`], e ogni file che arriva
/// rimanda in avanti la scadenza. Quel che si misura è quindi il **silenzio**,
/// non la durata: una libreria enorme su un disco sano non scade mai, una share
/// morta scade sempre.
///
/// # Un guasto non abortisce le altre radici
///
/// Un [`Passo::Caduta`] — la camminata che restituisce un errore invece di
/// finire — vale [`Passeggiata::Arenata`], cioè «di questa radice non si è
/// visto niente», e la scansione prosegue con le altre.
///
/// È un **cambio deliberato**: prima l'errore risaliva con un `?` fino a far
/// fallire l'intera scansione, e il risultato era che una cartella illeggibile
/// sul NAS impediva di scansionare anche il disco interno. La stessa regola che
/// vale per una radice spenta — «si salta quella, non si rinuncia a tutto» —
/// non aveva motivo di non valere anche qui.
///
/// # Il racconto
///
/// `conto` porta le cartelle e i file visti su tutte le radici, e `racconta`
/// lo riceve non più spesso di [`RACCONTO_CAMMINATA`]. Un `Break` da lì vale
/// quanto un Annulla: chi guarda ha detto basta.
fn cammina_una_radice(
    scan: &Scan<'_>,
    root: &str,
    conto: &mut ContoCammino,
    racconta: &mut dyn FnMut(usize, usize) -> ControlFlow<()>,
) -> Passeggiata {
    let disco = Arc::clone(&scan.files);
    let percorso = root.to_owned();
    let Some(passi) = crate::scadenza::a_rate("camminata", move |manda| {
        // La `send` che fallisce vuol dire che chi ascoltava se n'è andato:
        // niente da consegnare e niente da dire, si smette.
        let mut consegna = |voce| {
            let passo = match voce {
                Voce::File(file) => Passo::Trovato(file),
                Voce::RamoPerso(ramo) => Passo::RamoPerso(ramo),
                Voce::Cartella => Passo::Cartella,
            };
            if manda.send(passo).is_ok() {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        };
        let ultimo = match disco.walk_a_rate(&percorso, &mut consegna) {
            Ok(completa) => Passo::Fine { completa },
            Err(err) => Passo::Caduta(err),
        };
        let _ = manda.send(ultimo);
    }) else {
        // Il sistema non dà più fili: per chi chiama è la stessa cosa di una
        // radice che non risponde, e la reazione giusta è la stessa.
        return Passeggiata::Arenata;
    };

    let mut file = Vec::new();
    let mut rami = Vec::new();
    let mut ultima_consegna = Instant::now();
    // Il primo file ha diritto al doppio, e solo lui: il cronometro parte prima
    // che il produttore abbia fatto la sua prima `read_dir`, quindi quell'attesa
    // comprende l'apertura della radice — su una VPN lenta, un giro di rete che
    // col silenzio fra due file non c'entra niente. Dal secondo in poi si misura
    // quel che la scadenza vuole misurare, e vale il numero pieno.
    let mut margine = scan.scadenze.camminata.saturating_mul(2);
    loop {
        if scan.fermato() {
            return Passeggiata::Fermata;
        }
        if conto.ultimo_racconto.elapsed() >= RACCONTO_CAMMINATA {
            conto.ultimo_racconto = Instant::now();
            if racconta(conto.cartelle, conto.file).is_break() {
                return Passeggiata::Fermata;
            }
        }
        match passi.recv_timeout(BATTITO) {
            Ok(Passo::Trovato(trovato)) => {
                file.push(trovato);
                conto.file = conto.file.saturating_add(1);
                ultima_consegna = Instant::now();
                margine = scan.scadenze.camminata;
            }
            // Anche un ramo perso è una consegna: il disco ha risposto, con un
            // rifiuto. Il silenzio che si misura è quello di chi non risponde.
            Ok(Passo::RamoPerso(ramo)) => {
                rami.push(ramo);
                ultima_consegna = Instant::now();
                margine = scan.scadenze.camminata;
            }
            // E anche una cartella senza brani: mille cartelle di sole
            // copertine sono un disco che risponde, non uno che tace.
            Ok(Passo::Cartella) => {
                conto.cartelle = conto.cartelle.saturating_add(1);
                ultima_consegna = Instant::now();
                margine = scan.scadenze.camminata;
            }
            Ok(Passo::Fine { completa }) => {
                return Passeggiata::Vista {
                    file,
                    completa,
                    rami,
                };
            }
            Ok(Passo::Caduta(_)) => return Passeggiata::Arenata,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if ultima_consegna.elapsed() >= margine {
                    return Passeggiata::Arenata;
                }
            }
            // Il filo è finito senza dire come: o è caduto, o non è riuscito a
            // mandare l'ultimo passo. In nessuno dei due casi si sa di aver
            // visto tutto, quindi vale come arenata.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Passeggiata::Arenata,
        }
    }
}

/// Quel che la camminata ha contato finora, su tutte le radici.
struct ContoCammino {
    /// Le cartelle elencate: i battiti di [`Voce::Cartella`].
    cartelle: usize,
    /// I file trovati.
    file: usize,
    /// L'ultima volta che si è raccontato.
    ultimo_racconto: Instant,
}

/// Cosa si è visto guardando il disco, e cosa no.
#[derive(Debug, Default)]
pub struct Ricognizione {
    /// Tutti i file trovati, di tutte le radici che hanno risposto.
    pub trovati: Vec<DiscoveredFile>,
    /// Le radici su cui si è guardato davvero, e su cui quindi si può decidere.
    ///
    /// È l'elenco che va nel piano: una riga che non sta sotto nessuna di queste
    /// è «fuori competenza», non «sparita».
    pub viste: Vec<String>,
    /// Le radici che non hanno risposto, o che si sono arenate a metà.
    pub radici_saltate: Vec<String>,
    /// Le radici che la guardia per radice ha saltato perché vuote: vedi
    /// [`ScanReport::radici_vuote`]. Stanno anche in
    /// [`Ricognizione::radici_saltate`].
    pub radici_vuote: Vec<String>,
    /// Le cartelle illeggibili dentro radici che hanno risposto.
    ///
    /// La radice sta fra le [`Ricognizione::viste`] e si giudica, **tranne**
    /// quel che sta sotto uno di questi rami: lì non si è guardato, e una riga
    /// che non si è vista non è una riga sparita. Vedi [`Voce::RamoPerso`].
    pub rami_persi: Vec<String>,
    /// Qualcuno ha chiesto di smettere prima della fine.
    ///
    /// Quando è `true` le altre tre vanno lette come parziali: la camminata si è
    /// interrotta, e le radici che non si sono ancora guardate non sono né viste
    /// né saltate.
    pub fermata: bool,
    /// Quante cartelle si sono elencate: vedi [`ScanReport::cartelle_viste`].
    pub cartelle: usize,
}

/// Guarda tutte le radici, senza toccare il database.
///
/// È la prima metà di [`Scan::run_su`], estratta perché è anche la metà lunga:
/// su una libreria di rete può durare minuti, e in quei minuti nessun lucchetto
/// dev'essere in mano a nessuno.
///
/// # Le radici che non rispondono restano fuori
///
/// Una cartella su una condivisione spenta cammina «vuota»: la camminata non
/// fallisce, restituisce zero file. Passata così al dominio, quella radice
/// risulta **guardata e trovata deserta**, e ogni brano che ci stava sotto
/// diventa da cancellare, con dentro voti, preferiti e cronologia.
///
/// La correzione non tocca il dominio: le radici morte semplicemente non entrano
/// in [`Ricognizione::viste`]. Il meccanismo che serve c'è già — una riga che non
/// sta sotto nessuna radice sorvegliata è `untouched`, cioè «fuori competenza» —
/// ed è lo stesso che impedisce a una scansione della sola cartella dei download
/// di svuotare il resto della libreria.
pub fn cammina(scan: &Scan<'_>) -> Ricognizione {
    cammina_con(scan, &mut |_, _| ControlFlow::Continue(()))
}

/// Come [`cammina`], raccontando l'avanzamento: vedi [`Avanzamento::Camminata`].
///
/// # L'ordine
///
/// I file di ogni radice escono ordinati per chiave di percorso. La camminata
/// a più fili non promette un ordine (vedi `MusicFiles::walk_a_rate`), e chi
/// viene dopo sì: il piano, che mostrato due volte deve dire due volte la
/// stessa cosa; l'espansione delle immagini, che fra due fogli per la stessa
/// immagine sceglie sempre lo stesso; la lettura, che va a lotti, e un lotto
/// che cambia composizione fra due passate è un'interruzione che riprende da
/// un posto diverso.
pub(super) fn cammina_con(
    scan: &Scan<'_>,
    racconta: &mut dyn FnMut(usize, usize) -> ControlFlow<()>,
) -> Ricognizione {
    let mut esito = Ricognizione::default();
    let mut conto = ContoCammino {
        cartelle: 0,
        file: 0,
        ultimo_racconto: Instant::now(),
    };
    for root in scan.roots {
        if scan.fermato() {
            esito.fermata = true;
            esito.cartelle = conto.cartelle;
            return esito;
        }
        if !scan.files.radice_raggiungibile(root) {
            esito.radici_saltate.push(root.clone());
            continue;
        }
        match cammina_una_radice(scan, root, &mut conto, racconta) {
            Passeggiata::Fermata => {
                esito.fermata = true;
                esito.cartelle = conto.cartelle;
                return esito;
            }
            // Saltata **senza risondare**, e la differenza conta. Il gate qui
            // sotto esiste perché una camminata vuota o monca può essere una
            // share morta, e la sonda lo dice; una camminata arenata invece è
            // già la prova, e la sonda a quel punto è una domanda a cui il
            // sistema risponde male: la cartella si apre — dalla cache, o
            // perché è solo il contenuto a non arrivare — e il «sì» farebbe
            // passare per completo un elenco che non è nemmeno finito.
            Passeggiata::Arenata => {
                esito.radici_saltate.push(root.clone());
                continue;
            }
            Passeggiata::Vista {
                mut file,
                completa,
                mut rami,
            } => {
                // La rete può cadere **fra** la sonda e la camminata, e in quel
                // caso la camminata non lo dice da sé: torna vuota, o torna
                // monca. Le due si trattano uguali, e si risonda: se adesso la
                // radice non risponde più, quel vuoto non era un vuoto e quella
                // metà non era la libreria.
                //
                // Il caso monco è il più insidioso dei due, perché ha l'aria di
                // essere andato bene: su una libreria grande la share fa in
                // tempo a rispondere per i primi mille file e a morire sugli
                // altri centomila, e quei centomila — senza questo controllo —
                // diventano «spariti», cioè da cancellare, con dentro voti,
                // preferiti e cronologia.
                //
                // La sonda resta il giudice della radice, e non `completa` da
                // sé: una sottocartella a permessi negati sul disco di casa
                // abbassa `completa` esattamente come una share morta, ma
                // risponde alla sonda — e lì la scansione deve proseguire.
                if (!completa || file.is_empty()) && !scan.files.radice_raggiungibile(root) {
                    esito.radici_saltate.push(root.clone());
                    continue;
                }
                // Prosegue, ma **non come ha sempre fatto**. Fino alla 2.4 una
                // radice viva con un ramo perso si giudicava per intero, e ogni
                // brano sotto la sottocartella illeggibile risultava sparito:
                // una scansione a mano lo toglieva, con cronologia, playlist,
                // voti e preferiti. Adesso i rami persi viaggiano per nome, e
                // quel che ci sta sotto resta non verificato — vedi
                // `risparmia_i_non_verificati`.
                //
                // Una camminata che si dice monca senza aver detto dove ha
                // perso qualcosa ha rotto il patto di `walk_a_rate`: la si
                // prende in parola sulla sola cosa che ha detto, e il ramo
                // perso è la radice intera. Costa una passata senza rimozioni
                // su quella radice, che è il prezzo giusto di non sapere.
                if !completa && rami.is_empty() {
                    rami.push(root.clone());
                }
                file.sort_by_cached_key(|trovato| path_key(&trovato.path, scan.rules));
                esito.rami_persi.extend(rami);
                esito.trovati.extend(file);
                esito.viste.push(root.clone());
            }
        }
    }
    // L'ultimo racconto, a camminata finita: il numero che la finestra lascia
    // sullo schermo mentre comincia la fase dopo è quello vero.
    let _ = racconta(conto.cartelle, conto.file);
    esito.cartelle = conto.cartelle;
    esito
}

/// Un avviso del sistema su un percorso sotto le radici.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cambiamento {
    /// Il percorso che il sistema ha nominato.
    pub percorso: String,
    /// Creato, o rinominato **verso** questo nome: se adesso è una cartella,
    /// la si guarda da sé invece che dal genitore.
    pub nuovo: bool,
}

/// Che cos'è un percorso, adesso, sul disco.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Natura {
    /// Una cartella: si può camminare.
    Cartella,
    /// Un file.
    File,
    /// Niente, o niente che si riesca a vedere.
    Assente,
}

/// Le cartelle da riscansionare dopo che sotto le radici è cambiato qualcosa.
///
/// # A chi serve
///
/// Alla sorveglianza delle cartelle sul desktop: il sistema dice «questo file è
/// comparso», «quella cartella è stata rinominata», e da lì si vuole
/// riscansionare **il meno possibile** — un album, non la libreria. Una
/// scansione con un sottoalbero come radice fa già esattamente quel che serve:
/// giudica solo quel che sta sotto, e quel che sta fuori resta «fuori
/// competenza» (`ScanPlan::untouched`), cioè non può sparire. È lo stesso
/// meccanismo che tiene al sicuro il resto della libreria quando si scansiona
/// la sola cartella dei download, e non serve un altro modo di scansionare.
///
/// # Quale cartella, per ogni cambiamento
///
/// **Una cartella nuova si guarda da sé.** Un album copiato sul NAS arriva
/// come «è comparsa la cartella `…\Guccini\Radici`» (o «è stata rinominata
/// verso»): camminare quella basta a vedere tutto quel che contiene, e
/// camminare il genitore vorrebbe dire rileggere l'elenco di tutti gli album
/// dell'artista per trovarne uno. Lo dice [`Cambiamento::nuovo`] insieme alla
/// risposta del disco: dev'essere nuova **e** una cartella adesso.
///
/// **Tutto il resto, dalla cartella che lo contiene**, e non dal percorso
/// stesso: un file tolto non c'è più, e una cartella rinominata ha due nomi, di
/// cui uno non esiste più. Il genitore c'è — di solito — e camminandolo si vede
/// tanto quel che è arrivato quanto quel che se n'è andato.
///
/// Di solito, perché anche il genitore può essere sparito: si cancella un
/// artista intero, e il sistema racconta prima i file dei suoi album. Allora si
/// risale finché si trova qualcosa che è una [`Natura::Cartella`] — al più la
/// radice stessa. Non è pignoleria: una scansione su una cartella che non esiste
/// più la salta come una radice che non risponde, cioè **non toglie niente**
/// (vedi [`cammina`]), e i brani cancellati resterebbero in libreria per
/// sempre. E nemmeno un file va bene come punto d'arrivo: una «cartella» che è
/// diventata un file — rinominata, ricreata con lo stesso nome — non si
/// cammina.
///
/// Un percorso fuori da tutte le radici non produce niente; la radice stessa
/// cambiata produce la radice.
///
/// # Il minimo indispensabile
///
/// Cartelle una dentro l'altra si riducono alla più esterna: camminarla
/// comprende già l'altra, e due passate sulla stessa musica sono due letture
/// della stessa cartella. L'ordine dell'uscita è quello delle chiavi di
/// percorso, così due chiamate con gli stessi ingressi danno la stessa
/// scansione.
///
/// `natura` è una domanda al disco passata da fuori: questa funzione resta
/// pura, e chi la chiama sa se il disco è locale — dove chiederlo non costa
/// niente — o no.
///
/// Chi chiama — la sorveglianza — **non** passa gli avvisi di semplice
/// modifica su una cartella: arrivano già quelli dei file che contiene, e
/// guardarla da sé vorrebbe dire rileggerla tutta per un tag cambiato.
#[must_use]
pub fn cartelle_da_riguardare(
    cambiati: &[Cambiamento],
    radici: &[String],
    rules: PathRules,
    natura: &dyn Fn(&str) -> Natura,
) -> Vec<String> {
    let mut candidate: Vec<String> = Vec::new();
    for cambiamento in cambiati {
        let cambiato = cambiamento.percorso.as_str();
        let Some(radice) = radici.iter().find(|r| is_under(cambiato, r, rules)) else {
            continue;
        };
        let stessa = |a: &str, b: &str| path_key(a, rules) == path_key(b, rules);
        if dentro_una_cartella_di_sistema(cambiato, radice, rules) {
            continue;
        }
        let mut cartella = if stessa(cambiato, radice) {
            radice.clone()
        } else if cambiamento.nuovo && natura(cambiato) == Natura::Cartella {
            cambiato.to_owned()
        } else {
            cartella_che_contiene(cambiato).unwrap_or_else(|| radice.clone())
        };
        // Si risale finché si trova una cartella, senza mai uscire dalla radice.
        while !stessa(&cartella, radice) {
            if !is_under(&cartella, radice, rules) {
                cartella.clone_from(radice);
                break;
            }
            if natura(&cartella) == Natura::Cartella {
                break;
            }
            cartella = cartella_che_contiene(&cartella).unwrap_or_else(|| radice.clone());
        }
        // La radice si scrive come l'ha scritta chi l'ha aggiunta: è con quella
        // grafia che il piano confronta le maiuscole di quel che sta sotto.
        if stessa(&cartella, radice) {
            cartella.clone_from(radice);
        }
        candidate.push(cartella);
    }
    piu_esterne(candidate, rules)
}

/// Le parti di `cartella` che stanno sotto una radice sorvegliata.
///
/// La cartella stessa se sta dentro una radice; le radici che le stanno dentro
/// se è lei a contenerne; niente se le due non si incontrano. Serve alla
/// scansione dopo i download: limitata alla cartella dove i file sono arrivati,
/// ma mai fuori dalle radici — un file che nessuna scansione completa
/// troverebbe non deve entrare in libreria per la porta di servizio.
#[must_use]
pub fn dentro_le_radici(cartella: &str, radici: &[String], rules: PathRules) -> Vec<String> {
    if radici.iter().any(|r| is_under(cartella, r, rules)) {
        return vec![cartella.to_owned()];
    }
    let dentro: Vec<String> = radici
        .iter()
        .filter(|r| is_under(r, cartella, rules))
        .cloned()
        .collect();
    piu_esterne(dentro, rules)
}

/// La cartella che contiene un percorso, per com'è scritto: tutto quel che sta
/// prima dell'ultimo separatore. `None` per un percorso senza separatori.
///
/// Sul testo e non con `Path::parent`: questo modulo gira anche dove i
/// separatori del sistema non sono quelli di Windows, e i percorsi della
/// libreria sono stringhe scritte dalla camminata.
/// `percorso` passa, sotto `radice`, da una cartella di sistema del volume?
///
/// La camminata non ci entra ([`crate::files::e_cartella_di_sistema`]), ma la
/// radice si cammina per quel che è: un avviso dal Cestino di un'unità intera
/// sorvegliata — un brano appena eliminato, `D:\$RECYCLE.BIN\<SID>\$R….flac`
/// — farebbe una scansione con radice `D:\$RECYCLE.BIN\<SID>`, e il brano
/// tornerebbe in libreria. Si guardano solo i nomi **sotto** la radice: quella
/// l'ha scelta l'utente.
fn dentro_una_cartella_di_sistema(percorso: &str, radice: &str, rules: PathRules) -> bool {
    let mut qui = Some(percorso.to_owned());
    while let Some(cartella) = qui {
        if path_key(&cartella, rules) == path_key(radice, rules)
            || !is_under(&cartella, radice, rules)
        {
            return false;
        }
        let nome = cartella
            .trim_end_matches(['\\', '/'])
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or_default();
        if crate::files::e_cartella_di_sistema(nome) {
            return true;
        }
        qui = cartella_che_contiene(&cartella);
    }
    false
}

pub(super) fn cartella_che_contiene(percorso: &str) -> Option<String> {
    let senza_coda = percorso.trim_end_matches(['\\', '/']);
    let taglio = senza_coda.rfind(['\\', '/'])?;
    let testa = senza_coda.get(..taglio)?;
    // `C:\a` → `C:\`, non `C:`: la radice di un'unità ha il suo separatore.
    if testa.ends_with(':') {
        return senza_coda.get(..=taglio).map(str::to_owned);
    }
    (!testa.is_empty()).then(|| testa.to_owned())
}

/// Toglie i doppioni e le cartelle che stanno dentro un'altra dell'elenco.
///
/// Le cartelle restano scritte come sono arrivate, in ordine di chiave di
/// percorso. Pubblica perché chi mette insieme cartelle da più strade — gli
/// avvisi del sistema e il sondaggio delle cartelle di rete — deve ridurle
/// allo stesso modo prima di farne le radici di una scansione.
#[must_use]
pub fn piu_esterne(mut cartelle: Vec<String>, rules: PathRules) -> Vec<String> {
    use rymembra_domain::paths::path_key;
    cartelle.sort_by_key(|c| path_key(c, rules));
    let mut tenute: Vec<String> = Vec::new();
    for cartella in cartelle {
        if !tenute.iter().any(|t| is_under(&cartella, t, rules)) {
            tenute.push(cartella);
        }
    }
    tenute
}
