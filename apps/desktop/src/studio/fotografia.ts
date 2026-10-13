/**
 * La fotografia della tela, per la chat che vede l'anteprima.
 *
 * Nessuna dipendenza e niente di nativo: il documento della tela si copia in
 * un SVG con un `foreignObject`, con fogli, font e immagini ridotti a data
 * URI (un'immagine SVG non carica niente da fuori), e l'SVG si disegna su un
 * canvas. La tela mostra solo i dati finti di `mondo.ts`: nella fotografia
 * non finisce niente della libreria di chi la scatta.
 */

const LARGHEZZA_MASSIMA = 1280;
const QUALITA = 0.85;

/** Ferma tutto: la fotografia è la posa di riposo, non un fotogramma a caso. */
const FERMO = "*,*::before,*::after{animation:none!important;transition:none!important}";

const URL_CSS = /url\(\s*(?:"([^"]*)"|'([^']*)'|([^'")\s][^)]*?))\s*\)/g;

/** Un indirizzo che dentro un'immagine SVG non si caricherebbe mai. */
function daPortareDentro(indirizzo: string): boolean {
  return indirizzo !== "" && !indirizzo.startsWith("data:") && !indirizzo.startsWith("#");
}

/** Legge un indirizzo e lo restituisce come data URI; `null` se non si può. */
function inDataUri(indirizzo: string, cache: Map<string, Promise<string | null>>): Promise<string | null> {
  const giaChiesto = cache.get(indirizzo);
  if (giaChiesto) return giaChiesto;
  const chiesto = (async () => {
    try {
      const risposta = await fetch(indirizzo);
      if (!risposta.ok) return null;
      const dati = await risposta.blob();
      return await new Promise<string | null>((risolvi) => {
        const lettore = new FileReader();
        lettore.onload = () => risolvi(typeof lettore.result === "string" ? lettore.result : null);
        lettore.onerror = () => risolvi(null);
        lettore.readAsDataURL(dati);
      });
    } catch {
      return null;
    }
  })();
  cache.set(indirizzo, chiesto);
  return chiesto;
}

/**
 * Riscrive ogni `url(...)` di un testo CSS come data URI. Un indirizzo che
 * non si legge diventa `none`: meglio un fondo vuoto che un'immagine rotta.
 */
async function portaDentro(
  css: string,
  base: string,
  cache: Map<string, Promise<string | null>>,
): Promise<string> {
  const indirizzi = new Map<string, string>();
  for (const trovato of css.matchAll(URL_CSS)) {
    const scritto = (trovato[1] ?? trovato[2] ?? trovato[3] ?? "").trim();
    if (!daPortareDentro(scritto) || indirizzi.has(scritto)) continue;
    let assoluto: string;
    try {
      assoluto = new URL(scritto, base).href;
    } catch {
      continue;
    }
    indirizzi.set(scritto, assoluto);
  }
  const letti = new Map<string, string | null>();
  await Promise.all(
    [...indirizzi].map(async ([scritto, assoluto]) => {
      letti.set(scritto, await inDataUri(assoluto, cache));
    }),
  );
  return css.replace(URL_CSS, (tutto, doppio?: string, singolo?: string, nudo?: string) => {
    const scritto = (doppio ?? singolo ?? nudo ?? "").trim();
    if (!letti.has(scritto)) return tutto;
    const dato = letti.get(scritto);
    return dato ? `url("${dato}")` : "none";
  });
}

/** Tutti i fogli del documento, come testo, con le risorse già dentro. */
async function fogli(doc: Document, cache: Map<string, Promise<string | null>>): Promise<string> {
  const pezzi: Promise<string>[] = [];
  for (const foglio of doc.styleSheets) {
    let regole: CSSRuleList;
    try {
      regole = foglio.cssRules;
    } catch {
      // Un foglio di un'altra origine non si legge: la tela non ne ha, ma
      // un foglio illeggibile non deve far saltare la fotografia.
      continue;
    }
    let testo = "";
    for (const regola of regole) testo += regola.cssText + "\n";
    pezzi.push(portaDentro(testo, foglio.href ?? doc.baseURI, cache));
  }
  return (await Promise.all(pezzi)).join("\n") + FERMO;
}

/**
 * Cammina l'originale e la copia insieme e ripara nella copia quel che la
 * clonazione perde: canvas, valori dei campi, scorrimenti, immagini esterne.
 */
async function ripara(
  originale: Element,
  copia: Element,
  cache: Map<string, Promise<string | null>>,
  base: string,
): Promise<void> {
  const attese: Promise<void>[] = [];
  // Gli elementi della tela sono della finestra dell'iframe: `instanceof`
  // coi costruttori di qui darebbe sempre falso, quindi si guarda il nome.
  const cammina = (o: Element, c: Element) => {
    if (o.localName === "canvas") {
      const originaleCanvas = o as HTMLCanvasElement;
      const immagine = c.ownerDocument.createElement("img");
      for (const { name, value } of c.attributes) immagine.setAttribute(name, value);
      try {
        immagine.src = originaleCanvas.toDataURL("image/png");
      } catch {
        // Un canvas sporcato da fuori non si legge: resta un riquadro vuoto.
      }
      immagine.style.width = `${originaleCanvas.clientWidth}px`;
      immagine.style.height = `${originaleCanvas.clientHeight}px`;
      c.replaceWith(immagine);
      return;
    }
    if (o.localName === "input") {
      const campo = o as HTMLInputElement;
      if (campo.type === "checkbox" || campo.type === "radio") {
        if (campo.checked) c.setAttribute("checked", "");
        else c.removeAttribute("checked");
      } else {
        c.setAttribute("value", campo.value);
      }
    } else if (o.localName === "textarea") {
      c.textContent = (o as HTMLTextAreaElement).value;
    } else if (o.localName === "img") {
      c.removeAttribute("srcset");
      c.removeAttribute("loading");
      const figura = o as HTMLImageElement;
      const sorgente = figura.currentSrc || figura.src;
      if (daPortareDentro(sorgente)) {
        attese.push(
          inDataUri(sorgente, cache).then((dato) => {
            if (dato) c.setAttribute("src", dato);
            else c.removeAttribute("src");
          }),
        );
      } else if (sorgente) {
        c.setAttribute("src", sorgente);
      }
    }
    const stile = c.getAttribute("style");
    if (stile?.includes("url(")) {
      attese.push(portaDentro(stile, base, cache).then((nuovo) => c.setAttribute("style", nuovo)));
    }
    // Un contenitore scorso: la copia parte da capo, quindi i figli si
    // spostano di quanto l'originale era sceso.
    if (o.scrollTop !== 0 || o.scrollLeft !== 0) {
      (c as HTMLElement).style.overflow = "hidden";
      for (const figlio of c.children) {
        (figlio as HTMLElement).style.translate = `${-o.scrollLeft}px ${-o.scrollTop}px`;
      }
    }
    const figliO = o.children;
    const figliC = [...c.children];
    for (let i = 0; i < figliO.length; i++) {
      const fo = figliO[i];
      const fc = figliC[i];
      if (fo && fc) cammina(fo, fc);
    }
  };
  cammina(originale, copia);
  await Promise.all(attese);
}

/** Il primo fondo pieno fra corpo e radice: quel che il JPEG mette sotto. */
function fondo(vista: Window, doc: Document): string {
  for (const e of [doc.body, doc.documentElement]) {
    const colore = vista.getComputedStyle(e).backgroundColor;
    if (colore && colore !== "transparent" && !/^rgba\(.*,\s*0\)$/.test(colore)) return colore;
  }
  return "#000";
}

/** Aspetta il prossimo fotogramma, dopo chi ci si è messo in fila prima. */
function fotogramma(): Promise<void> {
  return new Promise((risolvi) => requestAnimationFrame(() => risolvi()));
}

/**
 * Fotografa la tela così com'è e la restituisce come JPEG in data URI, larga
 * al più `larghezzaMassima` pixel.
 */
export async function fotografaTela(
  cornice: HTMLIFrameElement,
  larghezzaMassima = LARGHEZZA_MASSIMA,
): Promise<string> {
  const doc = cornice.contentDocument;
  if (!doc?.body) throw new Error("la tela non ha ancora un documento");
  const radice = doc.documentElement;
  const larghezza = radice.clientWidth;
  const altezza = radice.clientHeight;
  if (larghezza === 0 || altezza === 0) throw new Error("la tela non ha ancora una misura");

  const cache = new Map<string, Promise<string | null>>();
  const css = await fogli(doc, cache);

  // Lo spettro disegna con WebGL senza tenere il buffer: si legge solo nello
  // stesso fotogramma in cui ha disegnato, cioè in una richiesta messa in
  // fila dopo la sua.
  await fotogramma();
  const copia = doc.body.cloneNode(true) as HTMLElement;
  await ripara(doc.body, copia, cache, doc.baseURI);

  // Il documento dell'immagine: dentro un SVG `:root` è lo `<svg>`, quindi
  // gli attributi della radice (skin, tema, densità, movimento) vanno sia
  // lì sia sull'`<html>` di dentro.
  const SVG = "http://www.w3.org/2000/svg";
  const XHTML = "http://www.w3.org/1999/xhtml";
  const carta = document.implementation.createDocument(SVG, "svg", null);
  const svg = carta.documentElement;
  const html = carta.createElementNS(XHTML, "html");
  for (const { name, value } of radice.attributes) {
    svg.setAttribute(name, value);
    html.setAttribute(name, value);
  }
  svg.setAttribute("width", String(larghezza));
  svg.setAttribute("height", String(altezza));
  svg.setAttribute("viewBox", `0 0 ${larghezza} ${altezza}`);
  const vista = doc.defaultView ?? window;
  svg.setAttribute("font-size", vista.getComputedStyle(radice).fontSize);
  const oggetto = carta.createElementNS(SVG, "foreignObject");
  oggetto.setAttribute("x", "0");
  oggetto.setAttribute("y", "0");
  oggetto.setAttribute("width", "100%");
  oggetto.setAttribute("height", "100%");
  const testa = carta.createElementNS(XHTML, "head");
  const stile = carta.createElementNS(XHTML, "style");
  stile.textContent = css;
  testa.append(stile);
  html.append(testa, carta.importNode(copia, true));
  oggetto.append(html);
  svg.append(oggetto);

  const testo = new XMLSerializer().serializeToString(carta);
  const immagine = new Image();
  immagine.decoding = "async";
  immagine.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(testo);
  await immagine.decode();

  const scala = Math.min(1, larghezzaMassima / larghezza);
  const tela = document.createElement("canvas");
  tela.width = Math.round(larghezza * scala);
  tela.height = Math.round(altezza * scala);
  const pennello = tela.getContext("2d");
  if (!pennello) throw new Error("niente canvas 2D");
  // Il JPEG non ha trasparenza: sotto, il fondo della finestra.
  pennello.fillStyle = fondo(vista, doc);
  pennello.fillRect(0, 0, tela.width, tela.height);
  pennello.drawImage(immagine, 0, 0, tela.width, tela.height);
  return tela.toDataURL("image/jpeg", QUALITA);
}

/** La tela dello Studio, se è montata. */
export function telaDelloStudio(): HTMLIFrameElement | null {
  return document.querySelector<HTMLIFrameElement>("iframe.finestra-tela");
}
