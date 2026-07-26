# Prompt per Claude Fable: Ottimizzazioni Mobile Thermal Management

## Obiettivo
Implementare un sistema completo di gestione termica e ottimizzazioni performace per l'app Aether su Android, prevenendo il surriscaldamento del dispositivo durante l'esecuzione di operazioni CPU-intensive come la scansione della libreria, l'enrichment dei metadati e il decoding audio.

## Contesto Tecnico
L'app Aether è un player musicale con architettura condivisa tra desktop (Electron) e mobile (Capacitor + nodejs-mobile). Il codice principale si trova in:
- `Aeter/electron/` - logica condivisa
- `Aeter - porting android/node-backend/` - backend Node.js per Android  
- `Aeter - porting android/src/lib/bridge.ts` - bridge WebView ↔ Node.js
- `Aeter - porting android/android/app/src/main/java/com/aether/player/NativeAudioPlugin.kt` - audio engine nativo

## Problemi Identificati

### 1. Library Scanner Troppo aggressivo (library.ts:319)
```typescript
const queue = new PQueue({ concurrency: 8 })
await queue.addAll(files.map(...))
```
**Problema**: 8 operazioni concorrenti su device mobile causano picchi CPU prolungati.

### 2. Nessun Controllo Termico
- Non esiste alcuna logica per monitorare la temperatura del dispositivo
- Operazioni non vengono sospese o ridotte quando il dispositivo si surriscalda

### 3. Audio Processing CPU-intensive (NativeAudioPlugin.kt)
- BiquadEQ processor applica cascade di filtri in tempo reale
- Crossfade con due player simultanei
- Nessun throttling dinamico basato su carico termico

### 4. Bridge JSON Overhead
- Ogni chiamata IPC serializza/deserializza JSON
- Molti eventi frequenti (timeupdate ogni 250ms) sprecano CPU

## Specifiche da Implementare

### Module 1: ThermalMonitor (Android - Kotlin)
**File**: `android/app/src/main/java/com/aether/player/ThermalMonitor.kt`

Implementa un sistema che:
- Monitora la temperatura tramite `TemperatureSensor` o stimolandola dal CPU usage
- Fornisce callback quando il dispositivo supera soglie (WARNING=55°C, CRITICAL=65°C)
- Integra con MediaPlaybackService per ridurre carico

### Module 2: AdaptiveConcurrencyManager (TypeScript/Node.js)
**File**: `electron/modules/adaptiveConcurrency.ts` (da creare)

Implementa:
```typescript
interface ThermalState {
  level: 'normal' | 'warning' | 'critical';
  temperature?: number;
  timestamp: number;
}

class AdaptiveConcurrencyManager {
  private thermalState: ThermalState = { level: 'normal', timestamp: Date.now() };
  
  // Metodi da implementare:
  updateThermalState(state: ThermalState): void;
  getOptimalConcurrency(baseConcurrency: number): number;
  shouldDeferOperation(operation: string): boolean;
}
```

### Module 3: Modified Library Scanner (library.ts)
**Modifica**: `electron/modules/library.ts`

Cambia da:
```typescript
const queue = new PQueue({ concurrency: 8 })
```

A un sistema dinamico che adatta la concorrenza in base al thermal state.

### Module 4: Audio Processor Thermal-Aware (NativeAudioPlugin.kt)
**Modifica**: `android/app/src/main/java/com/aether/player/NativeAudioPlugin.kt`

Aggiungi:
- Riduzione della frequenza di update EQ quando device è caldo
- Disabilitazione crossfade opzionale in modalità high-temp
- Log temperature samples per debugging

### Module 5: Bridge Optimization (bridge.ts)
**Modifica**: `src/lib/bridge.ts`

Implementa:
- Throttling degli eventi frequenti (timeupdate da 4Hz a 1Hz quando device caldo)
- Batch processing di messaggi IPC
- Compressione opzionale per payload grandi

### Module 6: Thermal Events Integration
**File**: `electron/ipc/thermal.ipc.ts` (da creare)

Registra handler IPC per:
- `thermal:update` - riceve aggiornamenti da Android ThermalMonitor
- `thermal:getState` - restituisce stato corrente
- `operation:deferred` - notifica quando operazione è stata deferita

## Integrazione con il Codice Esistente

### 1. In main.mobile.tsx (bootstrap)
Dopo l'installazione del bridge, registra listener per eventi termici:
```typescript
// DOPO installAetherBridge()
aether.on('thermal:update', (state) => {
  concurrencyManager.updateThermalState(state);
});
```

### 2. Nella creazione dei worker/background tasks
```typescript
// Prima di operazioni CPU-intensive:
if (concurrencyManager.shouldDeferOperation('library-scan')) {
  // Pianifica per dopo o riduci priorità
}
```

## Test e Verifica

### Unit Tests Richiesti
1. `test/thermalMonitor.test.ts` - test del ThermalMonitor
2. `test/adaptiveConcurrency.test.ts` - test dell'AdaptiveConcurrencyManager  
3. `electron/modules/libraryThermal.test.ts` - test della scanner con thermal awareness

### Test su Device Reale
- Verifica che la concorrenza si riduca da 8 a 2 quando temperatura > 55°C
- Verifica che operazioni vengano deferite quando temperatura > 65°C
- Misura differenza CPU usage con e senza thermal management

## Performance Targets

1. **Riduzione CPU usage**: almeno 40% durante scan su device caldo
2. **Temperatura massima**: non superare i 70°C in condizioni normali di utilizzo
3. **Responsività**: l'app non deve bloccare più di 2 secondi per aggiornamenti termici

## Dipendenze da Installare

Per Android (in build.gradle):
```gradle
implementation 'androidx.core:core:1.13.0' // per TemperatureSensor
```

Per TypeScript (package.json):
Nessuna nuova dipendenza richiesta - usare APIs esistenti.

## File da Modificare/Creare

### Da Creare:
1. `electron/modules/adaptiveConcurrency.ts`
2. `android/app/src/main/java/com/aether/player/ThermalMonitor.kt`
3. `electron/ipc/thermal.ipc.ts`
4. `test/adaptiveConcurrency.test.ts`
5. `test/thermalMonitor.test.ts`

### Da Modificare:
1. `electron/modules/library.ts` - adattare concurrency
2. `android/app/src/main/java/com/aether/player/NativeAudioPlugin.kt` - aggiungere thermal awareness
3. `src/lib/bridge.ts` - throttling eventi
4. `main.mobile.tsx` - integrare listener termici

## Struttura del Codice

### ThermalMonitor.kt (pseudocodice)
```kotlin
class ThermalMonitor(private val context: Context) {
    private var temperatureListeners = mutableListOf<(Double) -> Unit>()
    
    fun startMonitoring() {
        // Usa SensorManager per temperature o estima dal CPU usage
        // Every 5 seconds, check thermal status
    }
    
    fun addListener(callback: (temperature: Double) -> Unit) {
        temperatureListeners.add(callback)
    }
}
```

### AdaptiveConcurrency.ts (pseudocodice)
```typescript
export class AdaptiveConcurrencyManager {
  private state: ThermalState = 'normal';
  
  getConcurrency(base: number): number {
    switch(this.state) {
      case 'critical': return Math.max(1, Math.floor(base * 0.25));
      case 'warning': return Math.max(2, Math.floor(base * 0.5));
      default: return base;
    }
  }
}
```

## Note Importanti

⚠️ **NON modificare** il comportamento di fallback quando i sensori termici non sono disponibili - l'app deve funzionare anche su device senza temperature API.

⚠️ **NON bloccare** mai la UI o le operazioni principali - le deferenze devono essere asincrone e discrette.

✅ **MAINTENANCE**: Tutti i nuovi moduli devono avere JSDoc completo e commenti per future manutenzioni.

## Checklist Fine Implementation

- [ ] ThermalMonitor creato e testato
- [ ] AdaptiveConcurrencyManager integrato in library scanner  
- [ ] Audio processor ottimizzato
- [ ] Bridge throttling implementato
- [ ] IPC handlers registrati
- [ ] Unit tests scritti e passanti
- [ ] Documentazione aggiornata (README section "Mobile Performance")

---

**Prompt Status**: READY FOR CLaude Fable Implementation