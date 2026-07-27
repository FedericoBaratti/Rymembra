import { describe, expect, it } from 'vitest'
import {
  MIN_TRACK_BYTES,
  extensionOf,
  isInTrash,
  isSupportedAudioPath,
  isUnder,
  pathKey,
  type PathRules
} from './paths'
import { isNoOp, planScan, type DiscoveredFile, type KnownTrack } from './scanPlan'

const WINDOWS: PathRules = { caseInsensitive: true }
const POSIX: PathRules = { caseInsensitive: false }

function file(path: string, modifiedMs = 1000, sizeBytes = MIN_TRACK_BYTES): DiscoveredFile {
  return { path, sizeBytes, modifiedMs }
}

function track(id: number, path: string, modifiedMs = 1000): KnownTrack {
  return { id, path, modifiedMs }
}

describe('estensioni', () => {
  it('legge solo l\'ultimo segmento', () => {
    // Una cartella chiamata `Album.2019` non deve dare estensione a ciò che contiene.
    expect(extensionOf('C:/Musica/Album.2019/traccia')).toBe('')
    expect(extensionOf('C:/Musica/Album.2019/traccia.FLAC')).toBe('flac')
  })

  it('un nome che è solo un punto non ha estensione', () => {
    expect(extensionOf('.trashinfo')).toBe('')
    expect(extensionOf('/x/.gitignore')).toBe('')
  })

  it('riconosce i formati e rifiuta il resto', () => {
    expect(isSupportedAudioPath('a.mp3')).toBe(true)
    expect(isSupportedAudioPath('a.OPUS')).toBe(true)
    expect(isSupportedAudioPath('copertina.jpg')).toBe(false)
    expect(isSupportedAudioPath('note.txt')).toBe(false)
  })
})

describe('appartenenza a una cartella', () => {
  it('la cartella accanto non sta dentro', () => {
    /*
     * È il difetto costoso del vecchio albero: `C:\Musica` comincia con
     * `C:\Music`, quindi una scansione di `C:\Music` cancellava le righe della
     * cartella accanto. Serve il confine di separatore.
     */
    expect(isUnder('C:\\Musica\\a.mp3', 'C:\\Music', WINDOWS)).toBe(false)
    expect(isUnder('C:\\Music\\a.mp3', 'C:\\Music', WINDOWS)).toBe(true)
  })

  it('la cartella stessa sta dentro sé stessa', () => {
    expect(isUnder('C:/Music', 'C:/Music', WINDOWS)).toBe(true)
    expect(isUnder('C:/Music/', 'C:/Music', WINDOWS)).toBe(true)
  })

  it('barre miste e code di separatori non contano', () => {
    expect(isUnder('C:\\Music\\rock\\a.mp3', 'C:/Music/', WINDOWS)).toBe(true)
  })

  it('una radice vuota non contiene niente', () => {
    // Altrimenti una configurazione senza cartelle sorvegliate renderebbe «sotto
    // una radice» qualunque percorso, e la scansione svuoterebbe la libreria.
    expect(isUnder('C:/Music/a.mp3', '', WINDOWS)).toBe(false)
  })

  it('le maiuscole contano solo dove il filesystem le distingue', () => {
    expect(isUnder('/home/f/Musica/a.mp3', '/home/f/musica', POSIX)).toBe(false)
    expect(isUnder('C:/Musica/a.mp3', 'c:/MUSICA', WINDOWS)).toBe(true)
  })
})

describe('forma canonica', () => {
  it('unifica i separatori', () => {
    expect(pathKey('C:\\Music\\a.mp3', POSIX)).toBe('C:/Music/a.mp3')
  })

  it('piega il caso solo quando richiesto', () => {
    expect(pathKey('C:/Music/A.MP3', WINDOWS)).toBe('c:/music/a.mp3')
    expect(pathKey('C:/Music/A.MP3', POSIX)).toBe('C:/Music/A.MP3')
  })
})

describe('il cestino dei doppioni', () => {
  it('riconosce la cartella a qualunque profondità', () => {
    expect(isInTrash('C:/Download/.trash/a.mp3')).toBe(true)
    expect(isInTrash('C:/Download/.trash/2026/a.mp3')).toBe(true)
    expect(isInTrash('C:/Download/trash/a.mp3')).toBe(false)
    // Un nome che CONTIENE .trash non è la cartella .trash.
    expect(isInTrash('C:/Download/.trashcan/a.mp3')).toBe(false)
  })
})

describe('piano di scansione', () => {
  it('inserisce quel che non c\'era', () => {
    const plan = planScan(
      { roots: ['C:/Music'], found: [file('C:/Music/a.mp3')], known: [] },
      WINDOWS
    )
    expect(plan.toInsert.map((f) => f.path)).toEqual(['C:/Music/a.mp3'])
    expect(plan.toRemove).toEqual([])
    expect(isNoOp(plan)).toBe(false)
  })

  it('lascia stare quel che non è cambiato', () => {
    const plan = planScan(
      {
        roots: ['C:/Music'],
        found: [file('C:/Music/a.mp3', 5000)],
        known: [track(1, 'C:/Music/a.mp3', 5000)]
      },
      WINDOWS
    )
    expect(plan.unchanged).toBe(1)
    expect(isNoOp(plan)).toBe(true)
  })

  it('rilegge quel che ha cambiato data', () => {
    const plan = planScan(
      {
        roots: ['C:/Music'],
        found: [file('C:/Music/a.mp3', 7000)],
        known: [track(42, 'C:/Music/a.mp3', 5000)]
      },
      WINDOWS
    )
    expect(plan.toUpdate).toEqual([{ file: file('C:/Music/a.mp3', 7000), trackId: 42 }])
  })

  it('toglie quel che è sparito da una cartella scansionata', () => {
    const plan = planScan(
      { roots: ['C:/Music'], found: [], known: [track(1, 'C:/Music/a.mp3')] },
      WINDOWS
    )
    expect(plan.toRemove).toEqual([{ track: track(1, 'C:/Music/a.mp3'), reason: 'disappeared' }])
  })

  it('NON tocca le righe fuori dalle cartelle scansionate', () => {
    /*
     * Una scansione della sola cartella dei download non deve svuotare la
     * libreria di tutto il resto. È il caso che rende «non trovato» diverso da
     * «fuori competenza».
     */
    const plan = planScan(
      {
        roots: ['C:/Download'],
        found: [],
        known: [track(1, 'C:/Music/a.mp3'), track(2, 'C:/Download/b.mp3')]
      },
      WINDOWS
    )
    expect(plan.toRemove.map((r) => r.track.id)).toEqual([2])
    expect(plan.untouched).toBe(1)
  })

  it('la cartella con il nome che comincia uguale resta intatta', () => {
    // Lo stesso difetto di isUnder, visto dal piano: è qui che costava righe.
    const plan = planScan(
      { roots: ['C:/Music'], found: [], known: [track(1, 'C:/Musica/a.mp3')] },
      WINDOWS
    )
    expect(plan.toRemove).toEqual([])
    expect(plan.untouched).toBe(1)
  })

  it('una barra diversa non fa sparire un file che c\'è ancora', () => {
    /*
     * Il database ha il percorso con le barre rovesciate, la camminata lo
     * produce con quelle dritte. Un confronto esatto direbbe «sparito» e
     * «nuovo»: una cancellazione e un reinserimento, cioè la perdita di id,
     * conteggi di ascolto e appartenenza alle playlist.
     */
    const plan = planScan(
      {
        roots: ['C:\\Music'],
        found: [file('C:/Music/a.mp3', 5000)],
        known: [track(1, 'C:\\Music\\a.mp3', 5000)]
      },
      WINDOWS
    )
    expect(plan.toRemove).toEqual([])
    expect(plan.toInsert).toEqual([])
    expect(plan.unchanged).toBe(1)
  })

  it('e nemmeno una maiuscola, dove il filesystem non le distingue', () => {
    const plan = planScan(
      {
        roots: ['C:/Music'],
        found: [file('C:/music/A.MP3', 5000)],
        known: [track(1, 'C:/Music/a.mp3', 5000)]
      },
      WINDOWS
    )
    expect(plan.toRemove).toEqual([])
    expect(plan.unchanged).toBe(1)
  })

  it('ma dove le distingue, sono due brani', () => {
    const plan = planScan(
      {
        roots: ['/musica'],
        found: [file('/musica/A.mp3', 5000), file('/musica/a.mp3', 5000)],
        known: [track(1, '/musica/a.mp3', 5000)]
      },
      POSIX
    )
    expect(plan.toInsert.map((f) => f.path)).toEqual(['/musica/A.mp3'])
    expect(plan.unchanged).toBe(1)
    expect(plan.toRemove).toEqual([])
  })
})

describe('quel che non entra in libreria', () => {
  it('gli avanzi troncati', () => {
    // I lettori di metadati spesso ci riescono lo stesso: senza questa guardia
    // entrano come tracce normali e il guasto si scopre premendo play.
    const plan = planScan(
      {
        roots: ['C:/Music'],
        found: [file('C:/Music/mozzato.mp3', 1000, MIN_TRACK_BYTES - 1)],
        known: []
      },
      WINDOWS
    )
    expect(plan.toInsert).toEqual([])
    expect(plan.skipped.map((s) => s.reason)).toEqual(['tooSmall'])
  })

  it('quel che sta nel cestino dei doppioni', () => {
    // Senza, un file spostato lì dalla deduplicazione rientrerebbe da solo.
    const plan = planScan(
      { roots: ['C:/Download'], found: [file('C:/Download/.trash/a.mp3')], known: [] },
      WINDOWS
    )
    expect(plan.skipped.map((s) => s.reason)).toEqual(['inTrash'])
  })

  it('quel che non è audio', () => {
    const plan = planScan(
      { roots: ['C:/Music'], found: [file('C:/Music/copertina.jpg')], known: [] },
      WINDOWS
    )
    expect(plan.skipped.map((s) => s.reason)).toEqual(['nonAudio'])
  })

  it('un file scartato non fa sparire la sua riga', () => {
    /*
     * Il caso peggiore di tutti: il file è stato troncato da un download
     * interrotto. Se «scartato» contasse come «non trovato», la scansione
     * cancellerebbe la riga buona di un brano che l'utente ha ancora.
     */
    const plan = planScan(
      {
        roots: ['C:/Music'],
        found: [file('C:/Music/a.mp3', 1000, 10)],
        known: [track(1, 'C:/Music/a.mp3', 999)]
      },
      WINDOWS
    )
    expect(plan.skipped.map((s) => s.reason)).toEqual(['tooSmall'])
    expect(plan.toRemove).toEqual([])
    expect(plan.toUpdate).toEqual([])
  })

  it('lo stesso file trovato due volte nella stessa passata', () => {
    const plan = planScan(
      {
        roots: ['C:/Music'],
        found: [file('C:/Music/a.mp3'), file('C:\\Music\\A.MP3')],
        known: []
      },
      WINDOWS
    )
    expect(plan.toInsert).toHaveLength(1)
    expect(plan.skipped.map((s) => s.reason)).toEqual(['duplicate'])
  })
})

describe('righe doppie in libreria', () => {
  it('tiene la più vecchia e toglie l\'altra', () => {
    /*
     * Su un filesystem che ignora le maiuscole due righe possono passare il
     * vincolo di unicità sul percorso e nominare un file solo. Si tiene quella
     * con l'id più basso: è la più vecchia, quindi quella a cui playlist,
     * preferiti e conteggi di ascolto puntano più probabilmente.
     */
    const plan = planScan(
      {
        roots: ['C:/Music'],
        found: [file('C:/Music/a.mp3', 5000)],
        known: [track(7, 'C:/Music/A.mp3', 5000), track(3, 'C:/Music/a.mp3', 5000)]
      },
      WINDOWS
    )
    expect(plan.toRemove).toEqual([
      { track: track(7, 'C:/Music/A.mp3', 5000), reason: 'duplicateRow' }
    ])
    expect(plan.unchanged).toBe(1)
  })
})

describe('il piano si può mostrare prima di eseguirlo', () => {
  it('dice quanto sta per togliere', () => {
    /*
     * «Sto per togliere 340 brani» è la frase che l'utente deve poter leggere
     * quando ha staccato il disco esterno per sbaglio, invece di scoprirlo dopo.
     */
    const known: KnownTrack[] = []
    for (let i = 1; i <= 340; i++) known.push(track(i, `E:/Musica/${i}.mp3`))

    const plan = planScan({ roots: ['E:/Musica'], found: [], known }, WINDOWS)
    expect(plan.toRemove).toHaveLength(340)
    expect(plan.toRemove.every((r) => r.reason === 'disappeared')).toBe(true)
  })
})
