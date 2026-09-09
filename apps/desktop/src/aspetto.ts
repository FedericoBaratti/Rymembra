/**
 * Quel che si scrive sul documento: il foglio della skin, l'accento,
 * l'indirizzo di una copertina.
 *
 * # Perché non stanno in `ipc.ts`
 *
 * Perché non attraversano il confine con il nucleo. `ipc.ts` è il contratto:
 * una riga per ogni comando del processo nativo, e la forma di quel che torna.
 * Queste tre funzioni non chiamano niente — sostituiscono il testo di un
 * `<style>`, mettono e tolgono proprietà personalizzate sulla radice,
 * compongono un indirizzo che poi il motore di rendering chiederà da sé.
 * Toccano il documento, e il documento non è il nucleo.
 *
 * Stavano in fondo a `ipc.ts` perché è dove erano finite crescendo, e il prezzo
 * era che il confine non si leggeva più tutto d'un fiato: in coda ai comandi
 * c'era del codice che scrive nel DOM. Qui i due mestieri sono due file, e in
 * `git diff` si vede quale dei due sta cambiando.
 *
 * I tipi invece restano di là: `Skin` e `Variabile` sono quel che il nucleo
 * manda, quindi si descrivono dove si descrive il contratto. Da qui si
 * importano come tipi, e nient'altro passa in questa direzione.
 */
import type { Skin, Variabile } from "./ipc";

/**
 * I nomi scritti a mano sulla radice dall'ultimo accento dinamico.
 *
 * Fuori da React perché non è stato dell'interfaccia: è quel che c'è
 * sull'elemento, e serve solo a poterlo togliere. Tenerlo in uno `useState`
 * vorrebbe dire un disegno in più per una lista che nessuno guarda.
 */
let accentoScritto: readonly string[] = [];

/**
 * Scrive — o toglie — l'accento che segue la copertina.
 *
 * Sulla radice e non nel foglio della skin: così sopravvive a un cambio di
 * tema, e soprattutto **vince** sul foglio senza doverne toccare il testo.
 *
 * Con `null` rimette le cose com'erano. È il caso normale, non l'eccezione:
 * un disco senza copertina, una skin che non lo vuole, la preferenza spenta.
 */
export function applicaAccento(variabili: readonly Variabile[] | null): void {
  const radice = document.documentElement;
  // Prima si toglie quel che c'era: le variabili di un disco non sono
  // necessariamente le stesse del successivo — una skin può dichiarare
  // `--accent-soft` e un'altra no — e lasciarne indietro una vorrebbe dire un
  // accento mezzo vecchio e mezzo nuovo.
  for (const nome of accentoScritto) radice.style.removeProperty(nome);
  accentoScritto = [];
  if (!variabili) return;
  for (const { nome, valore } of variabili) radice.style.setProperty(nome, valore);
  accentoScritto = variabili.map((v) => v.nome);
}

/**
 * Applica una skin alla finestra.
 *
 * Un foglio a parte e non le proprietà scritte una a una su `style`: sostituire
 * il testo di un `<style>` è **un'unica** invalidazione per il motore di
 * rendering, mentre cinquanta `setProperty` sono cinquanta ricalcoli sull'intero
 * albero. Conta quando la skin cambierà dal vivo mentre la si costruisce.
 *
 * `data-skin` va messo dopo: il selettore del foglio è
 * `:root[data-skin='<id>']`, e metterlo prima significherebbe un fotogramma in
 * cui l'attributo c'è e le regole no.
 */
export function applicaSkin(skin: Skin): void {
  // Un accento tagliato sul contrasto della skin di prima non vale niente su
  // quella di adesso: si toglie subito, e chi guarda il brano in riproduzione
  // lo rimette con le superfici giuste. Vale anche per l'anteprima di una skin
  // che non è stata scelta, dove mostrare l'accento suo è la cosa onesta.
  applicaAccento(null);
  const id = "skin-attiva";
  const foglio =
    document.getElementById(id) ?? document.createElement("style");
  foglio.id = id;
  foglio.textContent = skin.css;
  if (!foglio.isConnected) document.head.append(foglio);
  const radice = document.documentElement;
  radice.dataset.skin = skin.id;
  // Le quattro scelte di impaginazione erano dichiarabili e non lette: il
  // formato le accettava, il compilatore ne scriveva una in un foglio che
  // nessuna regola interrogava, e le altre tre non uscivano nemmeno dal crate.
  // Da qui in giù sono attributi, quindi sono selettori, quindi contano.
  radice.dataset.player = skin.layout.player;
  radice.dataset.sidebar = skin.layout.sidebar;
  radice.dataset.density = skin.layout.density;
  radice.dataset.motion = skin.layout.motion;
}

/**
 * L'indirizzo di una copertina.
 *
 * Non passa dall'IPC: le immagini le chiede il motore di rendering al
 * protocollo `aether-cover`, in parallelo e con la sua cache. Novecento
 * copertine in base64 dentro delle risposte JSON sarebbero novecento stringhe
 * da tenere vive in memoria per disegnare dei quadratini.
 *
 * Su Windows il protocollo si raggiunge come `http://<schema>.localhost/…`.
 */
export function urlCopertina(
  hash: string | null,
  miniatura = true,
): string | null {
  if (!hash) return null;
  const nome = miniatura ? `${hash}.t` : hash;
  return `http://aether-cover.localhost/${nome}`;
}
