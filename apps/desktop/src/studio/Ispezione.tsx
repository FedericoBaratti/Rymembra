/**
 * La colonna di destra: l'ispettore di quel che si ha in mano.
 *
 * Una cosa sola decide che cosa si vede qui — la selezione — e ogni genere ha
 * il suo ispettore: una parte coi suoi quattro fogli (stile, stati,
 * movimento, codice), un token, un colore, un motivo, una curva,
 * un'animazione, un file, un carattere, un nodo dello scafale, il documento.
 * La chiave sul nome fa ripartire da capo i campi a metà quando si passa da
 * una voce all'altra: un nome battuto per metà su un colore non deve finire
 * sul colore dopo.
 */
import { IspezioneParte } from "./IspezioneParte";
import {
  IspezioneAnimazione,
  IspezioneCarattere,
  IspezioneColore,
  IspezioneCurva,
  IspezioneDocumento,
  IspezioneMotivo,
  IspezioneNodo,
  IspezioneRisorsa,
  IspezioneToken,
  IspezioneVuota,
} from "./IspezioneVoci";
import { useStudio } from "./contesto";

export function Ispezione() {
  const { s } = useStudio();
  const sel = s.selezione;
  if (sel === null) return <IspezioneVuota />;
  switch (sel.tipo) {
    case "parte":
      return <IspezioneParte key={sel.nome} parte={sel.nome} />;
    case "token":
      return <IspezioneToken key={sel.id} id={sel.id} />;
    case "colore":
      return <IspezioneColore key={sel.nome} nome={sel.nome} />;
    case "motivo":
      return <IspezioneMotivo key={sel.nome} nome={sel.nome} />;
    case "curva":
      return <IspezioneCurva key={sel.nome} nome={sel.nome} />;
    case "animazione":
      return <IspezioneAnimazione key={sel.nome} nome={sel.nome} />;
    case "risorsa":
      return <IspezioneRisorsa key={sel.nome} nome={sel.nome} />;
    case "carattere":
      return <IspezioneCarattere key={sel.nome} nome={sel.nome} />;
    case "nodo":
      return <IspezioneNodo key={sel.via.join("/")} via={sel.via} />;
    case "documento":
      return <IspezioneDocumento />;
  }
}
