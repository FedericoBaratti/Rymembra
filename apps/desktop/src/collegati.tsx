/**
 * Componenti della finestra collegati ai loro archivi.
 *
 * # Perché esistono
 *
 * `InRiproduzione`, `AvvisoAudio`, `ElencoBrani` e `Cartelle` prendono per prop
 * quel che disegnano, e devono continuare a farlo: lo Studio li disegna con
 * dati finti, passati a mano (`studio/Tela.tsx`). Ma nella finestra quei dati
 * stanno in un archivio (`riproduzione.ts`) o in un contesto (`Suona` in
 * `segnali.ts`), e farli passare da `App` voleva dire iscrivere `App` — cioè
 * ridisegnare la finestra intera a ogni pausa per accendere una riga.
 *
 * Qui ciascuno si iscrive da sé e passa la prop al componente vero. Il DOM è
 * quello del componente: un involucro di qui non disegna niente di suo.
 */
import type { ComponentProps, ReactNode } from "react";

import type { Avanzamento } from "./ipc";
import { useLibreria } from "./libreria";
import { AvvisoAudio } from "./parti/AvvisoAudio";
import { useSpecchioPresente } from "./presenza";
import { useLettore } from "./riproduzione";
import { Cartelle, type ManiDeiBrani } from "./schermate/Cartelle";
import { ElencoBrani } from "./schermate/Elenco";
import { InRiproduzione } from "./schermate/InRiproduzione";
import { useQuelCheSuona } from "./segnali";

/** «In riproduzione» a schermo intero, con lo stato del motore dall'archivio. */
export function InRiproduzioneCollegata(
  props: Omit<ComponentProps<typeof InRiproduzione>, "stato">,
) {
  return <InRiproduzione {...props} stato={useLettore()} />;
}

/** L'avviso dell'uscita audio, con lo stato del motore dall'archivio. */
export function AvvisoAudioCollegato(
  props: Omit<ComponentProps<typeof AvvisoAudio>, "stato">,
) {
  return <AvvisoAudio {...props} stato={useLettore()} />;
}

/**
 * Un elenco di brani con la riga accesa su quel che suona.
 *
 * Dal contesto `Suona`, che cambia solo col brano o con la pausa: l'elenco si
 * ridisegna lì — le righe sono `memo`, e cambiano le due che si accendono e si
 * spengono — e non a ogni passo del volume.
 */
export function ElencoBraniCollegato(
  props: Omit<ComponentProps<typeof ElencoBrani>, "inAscolto" | "inPausa">,
) {
  const ora = useQuelCheSuona();
  return <ElencoBrani {...props} inAscolto={ora.brano} inPausa={ora.inPausa} />;
}

/** Le Cartelle, con la riga accesa del pannello dei brani come sopra. */
export function CartelleCollegate({
  mani,
  ...props
}: Omit<ComponentProps<typeof Cartelle>, "mani"> & {
  mani: Omit<ManiDeiBrani, "inAscolto" | "inPausa">;
}) {
  const ora = useQuelCheSuona();
  return (
    <Cartelle
      {...props}
      mani={{ ...mani, inAscolto: ora.brano, inPausa: ora.inPausa }}
    />
  );
}

/** La scansione così com'è: per `useSpecchioPresente`, che vuole una scelta. */
function intera(scansione: Avanzamento | null): Avanzamento | null {
  return scansione;
}

/**
 * L'avanzamento della scansione, per chi lo disegna.
 *
 * Un figlio-funzione e non un involucro per ciascuno: la scansione la leggono
 * il primo avvio, la pila dei toast, l'annuncio a chi ascolta lo schermo e le
 * Impostazioni, che hanno prop diverse e stanno in punti diversi della
 * finestra. Si ridisegna questo, quattro volte al secondo mentre una passata
 * gira, e quel che la funzione restituisce — non `App`.
 *
 * Dallo specchio e non da `useArchivio`: la barra entra ed esce dalla pila con
 * una transizione di vista. Vedi «La scansione e le transizioni» in
 * `libreria.ts`.
 */
export function ConScansione({
  children,
}: {
  children: (scansione: Avanzamento | null) => ReactNode;
}) {
  const scansione = useSpecchioPresente(useLibreria().scansione, intera);
  return <>{children(scansione)}</>;
}
