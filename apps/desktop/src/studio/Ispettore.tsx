/**
 * L'ispettore: una superficie alla volta, e solo quel che il formato accetta.
 *
 * # La prima regola, resa in controlli
 *
 * «Non si può scrivere quel che il formato non accetta.» Finora l'ispettore
 * aveva quattro campi di testo, e tre di quei quattro accettavano qualunque cosa
 * — compresa una che la validazione avrebbe rifiutato un istante dopo. Ora ogni
 * riga ha la forma del suo tipo: un cursore per una lunghezza, un segmentato per
 * un enum, un elenco chiuso per un riferimento. L'unico posto dove si scrive
 * liberamente resta la scheda «Documento», dove a rispondere è il parser.
 *
 * # I controlli non stanno più qui
 *
 * Sono nati in questo file e se ne sono andati in `controlli.tsx` quando è
 * arrivato l'editor dei token: un colore è un colore che stia in
 * `parts.x.textColor` o in `tokens.color.accent`, e due copie dello stesso
 * controllo sono due controlli che divergono. Qui resta la **tabella delle
 * proprietà di una parte**, che è la cosa che questo file sa.
 */
import type { EffettoRegistro, ParteRegistro, TokenRegistro } from "../ipc";
import { Segmentato } from "../parti/Segmentato";
import {
  Cursore,
  Parole,
  Riga,
  ValoreColore,
  ValoreLunghezza,
} from "./controlli";
import { Livelli } from "./Livelli";
import {
  coloreCosto,
  costoDi,
  costoPila,
  type Livello,
  type Motivi,
  nomeEffetto,
} from "./valori";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";
import { descrizioneParte } from "./vocabolario";

/** Gli stati di una parte, come li chiama il registro. */
export function stati(): readonly (readonly [string | null, string])[] {
  return [
    [null, t("studio.state.base")],
    ["hover", t("studio.state.hover")],
    ["active", t("studio.state.active")],
    ["focus", t("studio.state.focus")],
    ["disabled", t("studio.state.disabled")],
  ];
}

/** Gli angoli che un `chamfer` può tagliare. */
const ANGOLI = ["topLeft", "topRight", "bottomLeft", "bottomRight"] as const;

export function Ispettore({
  definizione,
  stato,
  onStato,
  valoreDi,
  scrivi,
  valoreBase,
  scriviBase,
  tokens,
  effetti,
  tavolozza,
  motivi,
  budget,
}: {
  definizione: ParteRegistro | null;
  stato: string | null;
  onStato: (stato: string | null) => void;
  valoreDi: (campo: string) => unknown;
  scrivi: (campo: string, valore: unknown) => void;
  /** Come i due sopra, ma sempre alla base: `layer` non ha una versione per stato. */
  valoreBase: (campo: string) => unknown;
  scriviBase: (campo: string, valore: unknown) => void;
  tokens: readonly TokenRegistro[];
  effetti: readonly EffettoRegistro[];
  tavolozza: Readonly<Record<string, string>>;
  /** I motivi dichiarati: si possono impilare, e portano il loro costo. */
  motivi: Motivi;
  budget: number;
}) {
  if (definizione === null) {
    return (
      <aside className="ispettore">
        <div className="niente-scelto">
          <p>{t("studio.part.none")}</p>
          <p className="sotto">{t("studio.part.none.hint")}</p>
        </div>
      </aside>
    );
  }

  const livelli = (valoreDi("background") ?? []) as Livello[];
  const ritaglio = valoreDi("clip");
  const velo = valoreDi("filter");
  const sopra = (() => {
    const dichiarato = valoreBase("layer");
    if (dichiarato === null || typeof dichiarato !== "object") return [];
    const sfondi = (dichiarato as Record<string, unknown>)["background"];
    return Array.isArray(sfondi) ? (sfondi as Livello[]) : [];
  })();

  const costoVelo =
    velo === null || typeof velo !== "object"
      ? 0
      : costoDi(velo as Livello, effetti, motivi);
  // Il totale è la somma di quel che la superficie disegna davvero: lo sfondo,
  // il livello su `::after` e la sfocatura del fondo. Prima contava solo la
  // prima delle tre, quindi una parte poteva sforare il budget mostrando un
  // numero tranquillo.
  const costo =
    costoPila(livelli, effetti, motivi) +
    costoPila(sopra, effetti, motivi) +
    costoVelo;

  /**
   * Tutto quel che pesa, in un elenco solo.
   *
   * La barra mostrava soltanto lo sfondo, quindi il livello su `::after` e la
   * sfocatura contribuivano al totale senza comparire da nessuna parte: si
   * vedeva un numero rosso e nessun segmento che lo spiegasse.
   */
  const addendi = [
    ...livelli.map((livello) => ({
      livello,
      suo: costoDi(livello, effetti, motivi),
    })),
    ...sopra.map((livello) => ({
      livello,
      suo: costoDi(livello, effetti, motivi),
    })),
    ...(costoVelo > 0 ? [{ livello: velo as Livello, suo: costoVelo }] : []),
  ].filter(({ suo }) => suo > 0);
  const angoliTagliati = (() => {
    if (ritaglio === null || typeof ritaglio !== "object") return [];
    const quali = (ritaglio as Record<string, unknown>)["corners"];
    return Array.isArray(quali) ? quali.map(String) : [];
  })();

  return (
    <aside className="ispettore">
      <header className="testa-ispettore">
        <code className="nome-parte">.{definizione.name}</code>
        <span className="gruppo-parte">{definizione.group}</span>
        {definizione.layers && <span className="chip-livello">::after</span>}
        <p className="descrizione">
          {descrizioneParte(definizione.name, definizione.description)}
        </p>
      </header>

      <Segmentato
        etichetta={t("studio.part.state")}
        scelta={stato ?? "base"}
        onScegli={(s) => onStato(s === "base" ? null : s)}
        classe="minuto"
        voci={stati().map(([chiave, etichetta]) => ({
          chiave: chiave ?? "base",
          etichetta,
        }))}
      />

      <div className="corpo-ispettore">
        <Livelli
          livelli={livelli}
          effetti={effetti}
          tokens={tokens}
          tavolozza={tavolozza}
          motivi={motivi}
          budget={budget}
          onCambia={(nuovi) => scrivi("background", nuovi)}
        />

        <div className="proprieta">
          <Riga che={t("studio.part.text")}>
            <ValoreColore
              valore={valoreDi("textColor")}
              tokens={tokens}
              tavolozza={tavolozza}
              onCambia={(v) => scrivi("textColor", v)}
            />
          </Riga>

          <Riga che={t("studio.part.border")}>
            <div className="bordo">
              <ValoreColore
                valore={valoreDi("borderColor")}
                tokens={tokens}
                tavolozza={tavolozza}
                onCambia={(v) => scrivi("borderColor", v)}
              />
              <ValoreLunghezza
                valore={valoreDi("borderWidth")}
                min={0}
                max={8}
                passo={1}
                etichetta={t("studio.part.borderWidth")}
                onCambia={(v) => scrivi("borderWidth", v)}
              />
            </div>
          </Riga>

          <Riga che={t("studio.part.radius")}>
            <ValoreLunghezza
              valore={valoreDi("radius")}
              min={0}
              max={32}
              passo={1}
              etichetta={t("studio.part.radius.label")}
              onCambia={(v) => scrivi("radius", v)}
            />
          </Riga>

          <Riga che={t("studio.part.clip")}>
            <div className="ritaglio">
              <ValoreLunghezza
                valore={
                  ritaglio !== null && typeof ritaglio === "object"
                    ? ((ritaglio as Record<string, unknown>)["size"] ?? null)
                    : null
                }
                min={0}
                max={48}
                passo={1}
                etichetta={t("studio.part.clip.size")}
                onCambia={(v) =>
                  scrivi(
                    "clip",
                    v === null
                      ? null
                      : {
                          // Gli angoli già scelti sopravvivono alla misura. Prima
                          // no: cambiare la misura riscriveva l'oggetto da zero e
                          // i `corners` sparivano, cioè il taglio si allargava a
                          // tutti e quattro gli angoli mentre si stava toccando
                          // tutt'altro.
                          ...(ritaglio !== null && typeof ritaglio === "object"
                            ? (ritaglio as Record<string, unknown>)
                            : {}),
                          effect: "chamfer",
                          size: v,
                        },
                  )
                }
              />
              {/* Gli angoli si scelgono a griglia perché sono una griglia:
                  quattro caselle disposte come stanno sulla superficie si
                  leggono senza tradurre `bottomRight` in una posizione. */}
              <div
                className="angoli"
                role="group"
                aria-label={t("studio.part.corners")}
              >
                {ANGOLI.map((angolo) => {
                  const acceso = angoliTagliati.includes(angolo);
                  return (
                    <button
                      key={angolo}
                      type="button"
                      className="angolo"
                      aria-label={angolo}
                      aria-pressed={acceso}
                      data-active={acceso || undefined}
                      disabled={ritaglio === null || typeof ritaglio !== "object"}
                      onClick={() => {
                        const quali = acceso
                          ? angoliTagliati.filter((a) => a !== angolo)
                          : [...angoliTagliati, angolo];
                        scrivi("clip", {
                          ...(ritaglio as Record<string, unknown>),
                          // Nessun angolo dichiarato vuol dire tutti: togliere
                          // l'ultimo toglie il campo invece di scrivere una
                          // lista vuota, che il formato leggerebbe al contrario.
                          ...(quali.length === 0 ? { corners: undefined } : { corners: quali }),
                        });
                      }}
                    />
                  );
                })}
              </div>
            </div>
          </Riga>

          <Riga che={t("studio.part.opacity")}>
            <Cursore
              valore={valoreDi("opacity")}
              min={0}
              max={1}
              passo={0.01}
              etichetta={t("studio.part.opacity")}
              onCambia={(v) => scrivi("opacity", v)}
            />
          </Riga>

          <Riga che={t("studio.part.letters")}>
            {/* `em` e non `px`: la spaziatura fra le lettere segue la misura del
                testo, e in pixel fissi si sfalda appena il carattere cambia. */}
            <ValoreLunghezza
              valore={valoreDi("letterSpacing")}
              min={-0.1}
              max={0.5}
              passo={0.005}
              etichetta={t("studio.part.tracking")}
              unitaPredefinita="em"
              onCambia={(v) => scrivi("letterSpacing", v)}
            />
          </Riga>

          <Riga che={t("studio.part.caps")}>
            <Parole
              valore={valoreDi("textTransform")}
              etichetta={t("studio.part.transform")}
              onCambia={(v) => scrivi("textTransform", v)}
              voci={[
                { chiave: "uppercase", etichetta: "AB" },
                { chiave: "lowercase", etichetta: "ab" },
                { chiave: "capitalize", etichetta: "Ab" },
              ]}
            />
          </Riga>

          <Riga che={t("studio.part.weight")}>
            <Cursore
              valore={valoreDi("fontWeight")}
              min={100}
              max={900}
              passo={50}
              etichetta={t("studio.part.weight.label")}
              onCambia={(v) => scrivi("fontWeight", v)}
            />
          </Riga>

          {/* `filter` è l'undicesima proprietà di `PartAppearance`, e finora era
              l'unica senza un controllo: per sfocare quel che sta dietro una
              superficie bisognava scendere nel JSON. Costa dieci — l'intero
              budget di una superficie — e il numero sta accanto alla manopola
              invece che nel totale in fondo, che è la terza regola. */}
          <Riga che={t("studio.part.blurBehind")}>
            <div className="sfoca-dietro">
              <ValoreLunghezza
                valore={
                  velo !== null && typeof velo === "object"
                    ? ((velo as Record<string, unknown>)["radius"] ?? null)
                    : null
                }
                min={0}
                max={64}
                passo={1}
                etichetta={t("studio.part.blurRadius")}
                onCambia={(v) =>
                  scrivi(
                    "filter",
                    v === null ? null : { effect: "blurBehind", radius: v },
                  )
                }
              />
              <span
                className="peso"
                style={{ color: coloreCosto(costoVelo, budget) }}
                title={t("studio.part.blurCost")}
              >
                {costoVelo}
              </span>
            </div>
          </Riga>
        </div>

        {/* Lo pseudo-elemento libero, dove il registro dice che c'è. Il chip
            «::after» in testa all'ispettore lo annunciava già da un pezzo senza
            che ci fosse modo di scriverlo. */}
        {definizione.layers && (
          <Livelli
            livelli={sopra}
            effetti={effetti}
            tokens={tokens}
            tavolozza={tavolozza}
            motivi={motivi}
            budget={budget}
            titolo={t("studio.part.above")}
            onCambia={(nuovi) => {
              // `background` è obbligatorio dentro `layer`: un livello senza
              // sfondi non è un livello vuoto, è un oggetto che il parser
              // rifiuta. Toglierne l'ultimo toglie la dichiarazione intera.
              const dichiarato = valoreBase("layer");
              const opacita =
                dichiarato !== null && typeof dichiarato === "object"
                  ? (dichiarato as Record<string, unknown>)["opacity"]
                  : undefined;
              scriviBase(
                "layer",
                nuovi.length === 0
                  ? null
                  : opacita === undefined
                    ? { background: nuovi }
                    : { background: nuovi, opacity: opacita },
              );
            }}
          />
        )}

        <div className="costo">
          <div className="testa-costo">
            <span className="titolino">{t("studio.part.cost")}</span>
            <strong style={{ color: coloreCosto(costo, budget) }}>
              {costo}
            </strong>
            <span className="su">/ {budget}</span>
          </div>
          {/* Una barra proporzionale e non dieci caselle: un segmento per
              livello, largo quanto pesa. Così il livello che sta mangiando il
              budget si vede senza contare. */}
          <div className="misuratore" data-sopra={costo > budget || undefined}>
            {addendi.map(({ livello, suo }, indice) => (
              <span
                key={indice}
                style={{
                  width: `${Math.min(100, (suo / budget) * 100)}%`,
                  background: coloreCosto(suo, budget),
                }}
                title={nomeEffetto(livello, motivi) ?? "?"}
              />
            ))}
          </div>
          <p className="nota">
            {addendi.length === 0
              ? t("studio.part.cost.none")
              : addendi
                  .map(
                    ({ livello, suo }) =>
                      `${nomeEffetto(livello, motivi) ?? "?"} ${suo}`,
                  )
                  .join(" + ")}
          </p>
        </div>

        <div className="quel-che-non-ce section-card">
          <strong>{t("studio.part.absent")}</strong>
          <p>
            <Trans
              k="studio.part.absent.body"
              v={{ display: <code>display</code> }}
            />
          </p>
        </div>
      </div>
    </aside>
  );
}
