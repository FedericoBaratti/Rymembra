/**
 * Le app dell'utente presso Microsoft, Google e Dropbox, dal lato della
 * finestra: come si riconosce un identificativo scritto bene, quale app vale
 * quando nessuno sceglie, e che cosa vuol dire il guasto di un conto.
 *
 * # Perché le forme anche qui, se il nucleo le controlla già
 *
 * Il nucleo è l'autorità (`rymembra_domain::nuvola_app::difetto_id`), e
 * risponde `cloud.appIdInvalid` a un identificativo storto. Ma quella risposta
 * dice soltanto «non va», e arriva dopo il tasto: chi ha incollato l'ID della
 * directory al posto di quello dell'applicazione — due GUID uno sopra l'altro
 * nella stessa pagina di Entra — deve sentirsi dire **quale** campo e **perché**,
 * sotto il campo, prima che parta qualcosa. Le regole sono la copia esatta di
 * quelle Rust, prova per prova (`test/appNuvola.test.ts` ripete la tabella di
 * `nuvola_app.rs`): una regola più larga qui manderebbe al nucleo cose che lui
 * rifiuta, una più stretta rifiuterebbe ID buoni che il nucleo accetterebbe.
 *
 * Due dettagli che sembrano pedanteria e non lo sono: la lunghezza si conta in
 * **byte** UTF-8 (`str::len` del Rust), quella del nome in **caratteri**
 * (`chars().count()`), come fa il nucleo; e l'ordine dei difetti è il suo —
 * vuoto, poi troppo lungo, poi spazi — così il motivo detto qui è lo stesso
 * che il nucleo scriverebbe nel diario.
 *
 * # Niente React, niente lingue
 *
 * Come `selettoreNuvola.ts`: si prova da solo con `node --test`. Le funzioni
 * dicono il **tipo** di difetto; la frase la sceglie chi la mostra
 * (`parti/AppServizio.tsx`).
 */
import type {
  AppUtente,
  ChiavePiattaforma,
  ServizioNuvola,
  ServizioOfferto,
} from "./ipc";

// ── le forme ────────────────────────────────────────────────────────────────

/** Cosa non va in un identificativo, in un segreto o in un nome: `DifettoId` del nucleo. */
export type DifettoId = "vuoto" | "spazi" | "forma" | "lungo";

/** Oltre questa lunghezza, in byte, un identificativo o un segreto non è credibile. */
export const LUNGHEZZA_MASSIMA = 200;

/** Quanti caratteri può avere il nome di un'app. */
export const NOME_MASSIMO = 60;

/** Il suffisso di ogni identificativo di client Google. */
export const SUFFISSO_GOOGLE = ".apps.googleusercontent.com";

const CODIFICA = new TextEncoder();

/**
 * I difetti che valgono per tutti, sul testo già ripulito ai bordi.
 *
 * `\s` e non solo lo spazio: un a capo o una tabulazione **dentro** l'ID sono
 * quasi sempre un pezzo di riga copiato di troppo, ed è la stessa classe di
 * `char::is_whitespace`.
 */
function difettoComune(testo: string): DifettoId | null {
  if (testo.length === 0) return "vuoto";
  if (CODIFICA.encode(testo).length > LUNGHEZZA_MASSIMA) return "lungo";
  if (/\s/u.test(testo)) return "spazi";
  return null;
}

/** `8-4-4-4-12` cifre esadecimali, maiuscole o minuscole. */
function eUnGuid(id: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id);
}

/**
 * Cosa non va nell'identificativo dell'app, o `null` se ha la forma giusta.
 *
 * Si guarda il testo ripulito dei bordi: chi incolla porta spesso un a capo
 * con sé, e rifiutarlo per quello sarebbe pedanteria.
 *
 * - **Microsoft:** l'«ID applicazione (client)» è un GUID. Lo sono anche l'ID
 *   della directory e l'ID oggetto, nella stessa pagina: da qui non si
 *   distinguono, e lo dicono i passi.
 * - **Dropbox:** la «App key», da 8 a 32 lettere minuscole e cifre.
 * - **Google:** l'ID client finisce con `.apps.googleusercontent.com`, e
 *   prima ha lettere, cifre e trattini.
 */
export function difettoIdApp(piattaforma: ChiavePiattaforma, id: string): DifettoId | null {
  const pulito = id.trim();
  const comune = difettoComune(pulito);
  if (comune !== null) return comune;
  let buono: boolean;
  switch (piattaforma) {
    case "microsoft":
      buono = eUnGuid(pulito);
      break;
    case "dropbox":
      buono = /^[a-z0-9]{8,32}$/.test(pulito);
      break;
    case "google": {
      const testa = pulito.endsWith(SUFFISSO_GOOGLE)
        ? pulito.slice(0, -SUFFISSO_GOOGLE.length)
        : null;
      buono = testa !== null && /^[A-Za-z0-9-]+$/.test(testa);
      break;
    }
  }
  return buono ? null : "forma";
}

/**
 * Cosa non va nel segreto di un'app Google.
 *
 * Del formato non si giura niente — oggi comincia con `GOCSPX-`, ieri no —:
 * solo che ci sia, senza spazi dentro e di una lunghezza credibile, e senza
 * virgolette — le stesse di `VIRGOLETTE` nel nucleo —: copiato da un JSON o da
 * un messaggio, un segreto fra virgolette si salvava e il consenso falliva
 * dopo senza dire perché.
 */
export function difettoSegreto(segreto: string): DifettoId | null {
  const pulito = segreto.trim();
  return difettoComune(pulito) ?? (/["'`\u201c\u201d\u2018\u2019\u00ab\u00bb]/u.test(pulito) ? "forma" : null);
}

/**
 * Il nome di un'app ripulito, o `null` se non va.
 *
 * È quel che si legge nel menu «Con l'app»: non vuoto, su una riga, senza
 * caratteri di controllo, al più [`NOME_MASSIMO`] caratteri — caratteri e non
 * byte, come `nome_pulito` del nucleo: «Più» sono tre.
 */
export function nomePulito(nome: string): string | null {
  const pulito = nome.trim();
  const caratteri = [...pulito].length;
  if (caratteri === 0 || caratteri > NOME_MASSIMO || /\p{Cc}/u.test(pulito)) return null;
  return pulito;
}

// ── quale app ───────────────────────────────────────────────────────────────

/**
 * La piattaforma che rilascia l'app di un servizio, o `null` per chi non ne
 * vuole una (WebDAV, MEGA).
 */
export function piattaformaDi(servizio: ServizioNuvola): ChiavePiattaforma | null {
  switch (servizio) {
    case "onedrive":
      return "microsoft";
    case "gdrive":
      return "google";
    case "dropbox":
      return "dropbox";
    default:
      return null;
  }
}

/** Le app di una piattaforma, nell'ordine in cui sono state registrate. */
export function appDellaPiattaforma(
  app: readonly AppUtente[],
  piattaforma: ChiavePiattaforma | null,
): AppUtente[] {
  return piattaforma === null ? [] : app.filter((a) => a.piattaforma === piattaforma);
}

/**
 * L'app che vale quando nessuno sceglie: quella che il nucleo dice
 * predefinita, altrimenti la prima della piattaforma.
 *
 * Il ripiego sulla prima non è un'opinione: è quel che fa il nucleo per tutte
 * le piattaforme tranne Google, dove vince l'app del backup — e lì la dice già
 * `predefinita`.
 */
export function appPredefinita(
  app: readonly AppUtente[],
  piattaforma: ChiavePiattaforma | null,
): AppUtente | null {
  const sue = appDellaPiattaforma(app, piattaforma);
  return sue.find((a) => a.predefinita) ?? sue[0] ?? null;
}

/**
 * Il nome da proporre per un'app nuova: quello della piattaforma, e se c'è
 * già, «Google 2», «Google 3»…
 *
 * Il primo libero e non il conteggio più uno: chi ha tolto «Google 2» e tiene
 * «Google 3» si vede proporre «Google 2», che è quel che manca.
 */
export function nomeNuovo(app: readonly AppUtente[], nomePiattaforma: string): string {
  const presi = new Set(app.map((a) => a.nome.trim().toLocaleLowerCase()));
  if (!presi.has(nomePiattaforma.toLocaleLowerCase())) return nomePiattaforma;
  for (let n = 2; ; n += 1) {
    const proposta = `${nomePiattaforma} ${n}`;
    if (!presi.has(proposta.toLocaleLowerCase())) return proposta;
  }
}

/**
 * I servizi con la disponibilità che le app dicono adesso.
 *
 * Il nucleo la ricalcola a ogni `nuvola:app`, e la si rilegge; ma fra la
 * risposta di un salvataggio e la rilettura la linguetta appena configurata
 * mostrerebbe ancora il modulo dell'app. La regola è la stessa del nucleo —
 * un servizio col consenso nel browser è disponibile se la sua piattaforma ha
 * almeno un'app —, e chi non ne vuole una resta com'era.
 */
export function conDisponibili(
  servizi: readonly ServizioOfferto[],
  app: readonly AppUtente[],
): ServizioOfferto[] {
  return servizi.map((offerto) => {
    const piattaforma = piattaformaDi(offerto.servizio);
    if (piattaforma === null) return offerto;
    const disponibile = appDellaPiattaforma(app, piattaforma).length > 0;
    return disponibile === offerto.disponibile ? offerto : { ...offerto, disponibile };
  });
}

// ── chi la usa ──────────────────────────────────────────────────────────────

/** Chi si accorgerebbe di un cambio dell'app: i conti per nome, e il backup. */
export interface ChiLaUsa {
  /** I nomi dei conti legati, nell'ordine del nucleo. */
  conti: string[];
  /** È l'app del backup su Drive. */
  backup: boolean;
}

/** Vedi [`ChiLaUsa`]. */
export function chiLaUsa(app: AppUtente): ChiLaUsa {
  return {
    conti: app.usataDa.flatMap((uso) => (uso.conto === null ? [] : [uso.nome])),
    backup: app.usataDa.some((uso) => uso.conto === null),
  };
}

/**
 * L'identificativo come il nucleo lo salva e lo confronta
 * (`nuvola_app::forma_canonica`): ripulito ai bordi, e per Microsoft in
 * minuscolo, perché un GUID non distingue le maiuscole.
 */
export function idCanonico(piattaforma: ChiavePiattaforma, id: string): string {
  const pulito = id.trim();
  return piattaforma === "microsoft" ? pulito.toLowerCase() : pulito;
}

/**
 * Salvare questo identificativo cambia l'app per chi la usa?
 *
 * Il confronto è quello del nucleo: l'ID nuovo nella sua forma canonica contro
 * quello salvato. Un'app nuova non cambia niente per nessuno.
 */
export function cambiaIdentificativo(app: AppUtente | null, id: string): boolean {
  return app !== null && idCanonico(app.piattaforma, id) !== app.clientId;
}

// ── il guasto di un conto ───────────────────────────────────────────────────

/**
 * Che cosa serve a un conto «da ricollegare».
 *
 * - `"scegliApp"`: non ha più un'app (`cloud.clientMissing`) — se ne sceglie
 *   una, o se ne registra una;
 * - `"correggiApp"`: l'app c'è ma non va — il fornitore la rifiuta
 *   (`cloud.appRejected`) o le manca il segreto (`cloud.appSecretMissing`) —,
 *   e ricollegare senza toccarla darebbe lo stesso guasto;
 * - `"ricollega"`: l'app va bene, e basta un consenso nuovo — l'ID è cambiato
 *   (`cloud.appChanged`), il consenso è scaduto o revocato, e ogni altro caso.
 *
 * Distinguere i primi due dal terzo è quel che toglie il giro a vuoto:
 * «Ricollega» su un'app rifiutata riapriva il browser per farsi rifiutare di
 * nuovo.
 */
export type Rimedio = "scegliApp" | "correggiApp" | "ricollega";

/** Vedi [`Rimedio`]. `codice` è `ErroreIpc.code`, o niente. */
export function rimedioDi(codice: string | null | undefined): Rimedio {
  switch (codice) {
    case "cloud.clientMissing":
      return "scegliApp";
    case "cloud.appRejected":
    case "cloud.appSecretMissing":
      return "correggiApp";
    default:
      return "ricollega";
  }
}

/** Il guasto di un conto chiede di sistemare (o scegliere) l'app, e non solo di ricollegarsi? */
export function serveLApp(codice: string | null | undefined): boolean {
  return rimedioDi(codice) !== "ricollega";
}
