/**
 * Quante barre disegna la scena, scelto guardandola.
 *
 * # Perché sta in un angolo e non nelle impostazioni
 *
 * Perché la differenza fra 64 e 512 non si immagina leggendo un numero in
 * un'altra schermata: si vede. Stava sopra la striscia a dieci barre, che non
 * c'è più — la scena si è presa lo schermo — e sta in basso a sinistra,
 * smorzato, perché è un comando che si usa una volta e poi si guarda quel che
 * ha fatto. Si accende al passaggio **e al fuoco da tastiera**: un comando
 * raggiungibile solo col mouse è un comando che una parte delle persone non
 * trova mai.
 *
 * # Perché le risoluzioni sono scritte qui
 *
 * Sono le stesse otto di `aether_play::RISOLUZIONI`, e una costante del motore
 * non attraversa l'IPC. Non è un doppione da tenere allineato a mano sperando
 * di non sbagliare: il comando risponde con quante bande sono **rimaste**,
 * quindi una linguetta che chiedesse un numero che non esiste tornerebbe
 * indietro sulla più vicina invece di rompere qualcosa.
 */
import { Segmentato, type Voce } from "./Segmentato";
import { t } from "../lingue";

/** Le risoluzioni della scena, come le dichiara il nucleo. */
const RISOLUZIONI = [8, 16, 32, 64, 128, 256, 512, 1024] as const;

const VOCI: readonly Voce<string>[] = RISOLUZIONI.map((quante) => ({
  chiave: String(quante),
  etichetta: String(quante),
}));

export function DettaglioSpettro({
  barre,
  onBarre,
}: {
  /** Quante barre disegna la scena adesso. */
  barre: number;
  onBarre: (quante: number) => void;
}) {
  return (
    <div className="dettaglio-spettro">
      <Segmentato
        voci={VOCI}
        scelta={String(barre)}
        onScegli={(chiave) => onBarre(Number(chiave))}
        etichetta={t("spectrum.bars")}
        classe="minuto"
      />
    </div>
  );
}
