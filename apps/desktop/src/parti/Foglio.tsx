/**
 * Il foglio che sale dal basso: i menu del telefono.
 *
 * Sul computer un menu nasce dal puntatore, piccolo, accanto alla cosa cliccata
 * (`Menu.tsx`). Sul telefono quel posto è sotto il dito, e un menu da duecento
 * pixel ancorato a un pollice è un menu che il pollice copre. Il foglio sale
 * invece dal bordo di sotto, dove arriva il pollice, largo quanto lo schermo,
 * con le voci alte un dito; in cima dice **su cosa** si sta agendo — la
 * copertina e il titolo — perché la riga premuta adesso è sotto di lui.
 *
 * # Come si chiude
 *
 * In cinque modi, e tutti e cinque scivolano giù invece di sparire:
 *
 * - toccando il velo sopra;
 * - con Indietro (`useSuIndietro`): la scala di `App` chiude prima lo schermo
 *   intero e poi il menu, quindi senza questo un menu aperto da «In
 *   riproduzione» avrebbe chiuso la schermata e lasciato il menu aperto;
 * - scegliendo una voce: la voce fa il suo mestiere subito, e il foglio scende;
 * - trascinandolo giù dalla maniglia o dalla testata. Solo da lì, e non dalle
 *   voci: il corpo del foglio scorre quando le voci sono tante, e un dito che
 *   scende sulle voci sta scorrendo, non chiudendo;
 * - quando si apre una finestrella: il foglio le starebbe sopra, e lei ha una
 *   domanda da fare (`quandoSiApreUnModale` in `modale.ts`).
 *
 * Lo smontaggio aspetta la fine della discesa (`dopoLaTransizione`), che col
 * movimento spento è subito.
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { useDito } from "../dito";
import { useVeloInerte } from "../finestrella";
import { dopoLaTransizione } from "../foglio";
import { elastico, esitoTrascinamento } from "../gesti";
import { useSuIndietro } from "../indietro";
import { quandoSiApreUnModale } from "../modale";
import { vibra } from "../vibra";

/** Chiude il foglio dall'interno, scivolando: lo usano le voci. */
const ContestoFoglio = createContext<() => void>(() => {});

/** La chiusura animata del foglio che contiene chi la chiede. */
export function useChiudiFoglio(): () => void {
  return useContext(ContestoFoglio);
}

/** Quanto si può tirare un foglio verso l'alto: poco, e sempre meno. */
const ELASTICO_SU = 24;

export function Foglio({
  etichetta,
  testata,
  classe,
  onChiudi,
  children,
}: {
  /** Il nome del dialogo, per chi non vede la testata. */
  etichetta: string;
  /** Su cosa si sta agendo: copertina, titolo, sottotitolo. */
  testata?: ReactNode;
  /** Una classe in più sul pannello, per le varianti. */
  classe?: string | undefined;
  /** Il foglio è sceso: lo si smonta. */
  onChiudi: () => void;
  children: ReactNode;
}) {
  const pannello = useRef<HTMLDivElement>(null);
  const velo = useRef<HTMLDivElement>(null);
  const [chiudendo, setChiudendo] = useState(false);
  const [trascinando, setTrascinando] = useState(false);
  const ultimaChiusura = useRef(onChiudi);
  ultimaChiusura.current = onChiudi;

  const chiudi = useCallback(() => setChiudendo(true), []);
  useSuIndietro(chiudi);

  // Una finestrella che si apre da sola mentre il foglio è su — una richiesta
  // di collegamento, un link da un altro dispositivo — starebbe sotto di lui,
  // e Indietro la chiuderebbe senza che si sia vista: il foglio scende e la
  // lascia vedere. Vedi `quandoSiApreUnModale` in `modale.ts`.
  useEffect(() => quandoSiApreUnModale(chiudi), [chiudi]);

  // La discesa è cominciata: si smonta quando è finita.
  useEffect(() => {
    if (!chiudendo) return;
    return dopoLaTransizione(pannello.current, () => ultimaChiusura.current());
  }, [chiudendo]);

  // Chi aveva il fuoco, letto **prima** che la pagina diventi inerte: un nodo
  // reso inerte lo perde, e dopo si leggerebbe il `<body>`. Lo stesso ordine
  // di `Dialogo`, per la stessa ragione.
  const chiAveva = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    chiAveva.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }, []);

  // Il resto della finestra inerte, pannello escluso: `aria-modal` non basta,
  // e dietro il foglio TalkBack leggeva — e attivava col doppio tocco — le
  // stelle e i tasti della schermata sotto. Velo e pannello sono fratelli, e
  // il pannello resta vivo. Vedi `useVeloInerte`.
  useVeloInerte(velo, pannello);

  // Il fuoco entra nella prima voce, e torna dov'era alla chiusura: come il
  // menu del computer, e per la stessa ragione — vedi `Menu.tsx`. Dopo
  // l'inerzia, e anche alla chiusura: React smonta questi effetti nell'ordine
  // in cui sono scritti, quindi la pagina torna viva prima che il fuoco ci
  // ritorni.
  useLayoutEffect(() => {
    const daRidare = chiAveva.current;
    pannello.current
      ?.querySelector<HTMLElement>(".corpo-foglio button:not([aria-disabled='true'])")
      ?.focus({ preventScroll: true });
    return () => {
      if (daRidare !== null && daRidare !== document.body && daRidare.isConnected) {
        daRidare.focus({ preventScroll: true });
      }
    };
  }, []);

  /** Lo spostamento del dito, scritto sul pannello e non nello stato. */
  const sposta = (dy: number) => {
    const nodo = pannello.current;
    if (nodo === null) return;
    const giu = dy >= 0 ? dy : elastico(dy, ELASTICO_SU);
    nodo.style.setProperty("--trascina-y", `${giu}px`);
  };
  const oltre = useRef(false);
  const dito = useDito({
    assi: "y",
    onParte: () => {
      oltre.current = false;
      setTrascinando(true);
    },
    onMuovi: (_dx, dy) => {
      sposta(dy);
      // Una vibrazione quando si passa la soglia: lasciando adesso, si chiude.
      const altezza = pannello.current?.offsetHeight ?? 0;
      const passato = dy > altezza / 4;
      if (passato !== oltre.current) {
        oltre.current = passato;
        if (passato) vibra("scatto");
      }
    },
    onFine: ({ dy, vy }) => {
      setTrascinando(false);
      const altezza = pannello.current?.offsetHeight ?? 0;
      if (esitoTrascinamento({ dy, vy, altezza }) === "chiudi") {
        chiudi();
      } else {
        pannello.current?.style.removeProperty("--trascina-y");
      }
    },
    onAnnulla: () => {
      setTrascinando(false);
      pannello.current?.style.removeProperty("--trascina-y");
    },
  });

  return (
    <ContestoFoglio value={chiudi}>
      <div
        ref={velo}
        className="velo foglio-velo"
        data-chiudendo={chiudendo || undefined}
        aria-hidden="true"
        onClick={chiudi}
        onContextMenu={(e) => {
          // Un tocco lungo sul velo è un tocco sul velo: chiude, e non apre il
          // menu della riga che sta sotto.
          e.preventDefault();
          chiudi();
        }}
      />
      <div
        ref={pannello}
        className={classe === undefined ? "foglio" : `foglio ${classe}`}
        role="dialog"
        aria-modal="true"
        aria-label={etichetta}
        data-chiudendo={chiudendo || undefined}
        data-trascinando={trascinando || undefined}
      >
        <div className="presa-foglio" onPointerDown={dito.onPointerDown}>
          <span className="maniglia-foglio" aria-hidden="true" />
          {testata}
        </div>
        <div className="corpo-foglio">{children}</div>
      </div>
    </ContestoFoglio>
  );
}
