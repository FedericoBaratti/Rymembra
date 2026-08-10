/**
 * Lo spettro: dieci barre, e sono vere.
 *
 * # Perché era spento, e perché adesso no
 *
 * Il bottone c'era, spento, e il suggerimento diceva la verità: «il motore
 * audio non espone né campioni né bande, e disegnarne uno finto sarebbe
 * l'unica bugia dell'interfaccia». Adesso li espone — `aether-play::spettro`
 * prende i campioni dalla callback attraverso un terzo anello senza lucchetti
 * e li riduce alle dieci bande dell'equalizzatore — quindi il motivo per cui
 * era spento non c'è più. La regola non è cambiata: quel che si disegna qui
 * viene dal suono che esce davvero.
 *
 * # Perché una canvas e non dieci `div`
 *
 * Perché cambia trenta volte al secondo. Dieci elementi con un'altezza in
 * percentuale vorrebbero dire trecento scritture di stile al secondo, ognuna
 * con la sua invalidazione del layout; una canvas è un disegno solo, e il
 * lavoro non passa da React.
 *
 * # I colori vengono dai token
 *
 * `--viz-primary`, `--viz-secondary`, `--viz-glow`. Erano nel registro dal
 * primo giorno «letti da canvas che oggi non esistono»: questa è la canvas.
 * Si rileggono a ogni cambio di skin perché una skin può cambiarli, e si
 * rileggono da `getComputedStyle` perché è l'unico posto in cui il valore
 * risolto esiste — una canvas non conosce le proprietà personalizzate.
 */
import { useEffect, useRef } from "react";

import { ipc } from "../ipc";
import { listen } from "@tauri-apps/api/event";

/** Quante barre. Le stesse bande dell'equalizzatore. */
const BANDE = 10;

/** Il movimento del sistema è ridotto. */
function motoRidotto(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function Spettro({ onErrore }: { onErrore: (e: unknown) => void }) {
  const tela = useRef<HTMLCanvasElement>(null);
  /** Le bande più recenti, fuori da React: le legge solo il disegno. */
  const bande = useRef<number[]>(new Array<number>(BANDE).fill(0));

  // Accende la presa all'apertura e la spegne all'uscita. Senza lo spegnimento
  // la callback audio continuerebbe a riempire un anello che nessuno svuota, e
  // il filo di là a mandare trenta eventi al secondo a una schermata chiusa.
  useEffect(() => {
    ipc.spettro(true).catch(onErrore);
    const promessa = listen<number[]>("riproduzione:spettro", (evento) => {
      bande.current = evento.payload;
    });
    return () => {
      void promessa.then((stop) => stop());
      ipc.spettro(false).catch(onErrore);
    };
  }, [onErrore]);

  useEffect(() => {
    const canvas = tela.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const stile = getComputedStyle(canvas);
    const primario = stile.getPropertyValue("--viz-primary").trim() || "currentColor";
    const secondario = stile.getPropertyValue("--viz-secondary").trim() || primario;
    const alone = stile.getPropertyValue("--viz-glow").trim();

    let fotogramma = 0;
    let vivo = true;

    const disegna = () => {
      if (!vivo) return;
      // La misura vera in pixel del dispositivo: su uno schermo al 150% una
      // canvas dichiarata in CSS e disegnata in unità CSS esce sfocata.
      const scala = window.devicePixelRatio || 1;
      const larghezza = Math.round(canvas.clientWidth * scala);
      const altezza = Math.round(canvas.clientHeight * scala);
      if (canvas.width !== larghezza || canvas.height !== altezza) {
        canvas.width = larghezza;
        canvas.height = altezza;
      }
      ctx.clearRect(0, 0, larghezza, altezza);

      const passo = larghezza / BANDE;
      // Un quinto di spazio fra una barra e l'altra: meno e diventa un blocco
      // solo, di più e le barre sembrano più strette di quanto sono alte.
      const larga = passo * 0.62;
      const raggio = Math.min(larga / 2, 3 * scala);

      const sfumatura = ctx.createLinearGradient(0, altezza, 0, 0);
      sfumatura.addColorStop(0, secondario);
      sfumatura.addColorStop(1, primario);
      ctx.fillStyle = sfumatura;
      if (alone) {
        ctx.shadowColor = alone;
        ctx.shadowBlur = 10 * scala;
      }

      for (let i = 0; i < BANDE; i += 1) {
        const valore = bande.current[i] ?? 0;
        // Un minimo visibile anche a zero: dieci barre che spariscono del tutto
        // lasciano un rettangolo vuoto che sembra un guasto, e la riga di base
        // dice «c'è, e adesso è a zero».
        const alta = Math.max(valore * altezza, 2 * scala);
        const x = i * passo + (passo - larga) / 2;
        ctx.beginPath();
        ctx.roundRect(x, altezza - alta, larga, alta, raggio);
        ctx.fill();
      }

      // Con `prefers-reduced-motion` non si insegue ogni fotogramma: si
      // ridisegna quattro volte al secondo. Le barre restano vere e smettono
      // di muoversi come un'animazione — una preferenza di accessibilità non
      // si aggira accendendo proprio la cosa che vieta.
      if (motoRidotto()) {
        fotogramma = window.setTimeout(disegna, 250);
      } else {
        fotogramma = requestAnimationFrame(disegna);
      }
    };

    disegna();
    return () => {
      vivo = false;
      cancelAnimationFrame(fotogramma);
      clearTimeout(fotogramma);
    };
  }, []);

  return (
    <div className="viz-screen spettro">
      <div className="viz-title occhiello">Spettro</div>
      <canvas
        ref={tela}
        className="eq-bars"
        /* Il grafico non è leggibile da chi ascolta lo schermo, e non c'è un
           testo che lo sostituisca senza inventarlo: è decorazione che segue il
           suono, e l'informazione che porta — «sta suonando» — è già nel
           trasporto. */
        aria-hidden="true"
      />
    </div>
  );
}
