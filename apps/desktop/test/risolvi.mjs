/**
 * Gli import senza estensione, per `node --test`.
 *
 * I sorgenti scrivono `./patch` come vuole Vite; Node vuole il file. Un
 * gancio di risoluzione prova `.ts` e `.tsx` quando la forma nuda non si
 * trova — nessuna dipendenza, nessuna compilazione: i tipi li toglie Node.
 */
import { registerHooks } from "node:module";

registerHooks({
  resolve(specifier, context, nextResolve) {
    try {
      return nextResolve(specifier, context);
    } catch (errore) {
      if (!specifier.startsWith(".")) throw errore;
      for (const coda of [".ts", ".tsx", "/index.ts"]) {
        try {
          return nextResolve(specifier + coda, context);
        } catch {
          // la prossima
        }
      }
      throw errore;
    }
  },
});
