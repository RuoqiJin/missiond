;; ════════════════════════════════════════════════════════════════
;; MissionD Domain Engine Declarations
;; Pattern: domain-engine (zero internal logic, shell only)
;; Status: Phase 3 — core engine containment
;; ════════════════════════════════════════════════════════════════

;; --- Slot Orchestrator ---
;; Multi-engine agent lifecycle manager.
;; Routes tasks by CliEngine to sub-managers (Claude/Gemini/Codex).

(domain-engine slot-orchestrator
  :struct SlotOrchestrator
  :target "missiond-daemon/src/slot_orchestrator/gen_engine.rs"
  (depends-on
    (control "Arc<ControlManager>"))
  (state
    (registry "RwLock<HashMap<String, SlotTaskConfig>>" :default "RwLock::new(HashMap::new())")
    (managers "HashMap<CliEngine, Arc<dyn EngineSlotManager>>" :default "HashMap::new()"))
  (exports
    (fn register
      :params ((config SlotTaskConfig))
      :returns "Result<()>"
      :doc "Register a slot configuration for task dispatch")
    (fn execute
      :params ((task "&SlotTaskRequest"))
      :returns "Result<String>"
      :doc "Execute a task on the best available engine slot")
    (fn status
      :returns "Vec<EngineStatus>"
      :doc "Get status of all managed engine slots")
    (fn is_registered
      :params ((task_type "&str"))
      :returns bool
      :async false
      :doc "Check if a task type is registered")))

;; --- LLM Gateway ---
;; Multi-provider LLM dispatch with priority channels and rate limiting.

(domain-engine llm-gateway
  :struct LlmGateway
  :target "missiond-daemon/src/llm/gen_engine.rs"
  (depends-on
    (gemini "GeminiGateway")
    (sonnet "SonnetHandle")
    (minimax "Option<MinimaxHandle>"))
  (state
    (request_count "AtomicU64" :default "AtomicU64::new(0)"))
  (exports
    (fn route_interactive
      :params ((prompt "&str") (model "&str"))
      :returns "Result<String>"
      :doc "Route an interactive LLM request to the best provider")
    (fn route_embedding
      :params ((text "&str"))
      :returns "Result<Vec<f32>>"
      :doc "Generate embedding vector via the embedding channel")
    (fn route_translation
      :params ((text "&str") (target_lang "&str"))
      :returns "Result<String>"
      :doc "Route a translation request")))

;; --- Autopilot Tick Engine ---
;; Composite orchestrator tick loop: memory → extraction → task → decision → flow.

(domain-engine autopilot
  :struct Autopilot
  :target "missiond-daemon/src/engine/intent_engine/gen_engine.rs"
  (depends-on
    (db "DbExecutor")
    (slots "Arc<SlotOrchestrator>")
    (event_bus "EventBus")
    (llm "LlmGateway")
    (context "ContextPipeline"))
  (exports
    (fn tick
      :returns "Result<()>"
      :doc "Execute one full tick cycle: memory → extraction → task → decision → flow")
    (fn pause
      :returns "()"
      :doc "Pause the tick loop")
    (fn resume
      :returns "()"
      :doc "Resume the tick loop")))

;; --- Learning Engine ---
;; Decision cascade + knowledge extraction from conversation history.

(domain-engine learning-engine
  :struct LearningEngine
  :target "missiond-daemon/src/engine/learning_engine/gen_engine.rs"
  (depends-on
    (db "DbExecutor")
    (llm "LlmGateway")
    (event_bus "EventBus"))
  (state
    (extraction_phase "ExtractionPhase" :default "ExtractionPhase::Idle"))
  (exports
    (fn decide
      :params ((question "&AgentQuestion"))
      :returns "Result<Decision>"
      :doc "Route a question through KB → LLM → human cascade")
    (fn extract_memory
      :params ((session_id "&str"))
      :returns "Result<Vec<KnowledgeEntry>>"
      :doc "Extract knowledge entries from a conversation session")
    (fn tick
      :returns "Result<()>"
      :doc "Run one learning engine tick")))
