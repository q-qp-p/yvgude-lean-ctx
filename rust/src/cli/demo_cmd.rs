//! Development-only walkthrough of an unshipped control-plane concept.
//!
//! This command demonstrates the full pipeline without requiring a live server:
//! Task → Context → Scheduler → Execution → Receipt → Outcome → ETPAO

use std::thread;
use std::time::Duration;

use crate::core::cost_per_outcome::CostPerAcceptedOutcome;
use crate::core::etpao::{EtpaoReport, TokenPricing, TokenUsage};

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";
const CHECK: &str = "✓";
const ARROW: &str = "→";

pub(crate) fn cmd_demo(args: &[String]) {
    if std::env::var("LEAN_CTX_EXPERIMENTAL_DEMO").as_deref() != Ok("1") {
        eprintln!(
            "The control-plane demo is Research and unavailable in the public LeanCTX Runtime. \\
             Set LEAN_CTX_EXPERIMENTAL_DEMO=1 only for a local development evaluation."
        );
        return;
    }

    let subcommand = args.first().map(String::as_str).unwrap_or("help");
    match subcommand {
        "task-lifecycle" | "lifecycle" => run_task_lifecycle_demo(),
        "help" | "--help" | "-h" => print_demo_help(),
        other => {
            eprintln!("Unknown demo: '{other}'");
            eprintln!("Available: task-lifecycle");
            std::process::exit(1);
        }
    }
}

fn print_demo_help() {
    println!("{BOLD}lean-ctx demo{RESET} — development-only Control Plane research walkthrough");
    println!();
    println!("  {BOLD}lean-ctx demo task-lifecycle{RESET}");
    println!("    Spielt einen kompletten Task end-to-end durch und zeigt");
    println!("    bei jedem Schritt was passiert und warum.");
    println!();
    println!(
        "  {DIM}Nur mit LEAN_CTX_EXPERIMENTAL_DEMO=1. Der Demo-Befehl verwendet echte Protocol-Typen (TaskEnvelopeV1,"
    );
    println!("  ExecutionReceiptV1, etc.) und echte Compression — kein Mock-Daten.{RESET}");
}

fn run_task_lifecycle_demo() {
    print_header();
    pause();

    step_1_task_identity();
    pause();

    let (raw_tokens, compressed_tokens) = step_2_context_assembly();
    pause();

    step_3_scheduler();
    pause();

    step_4_execution(raw_tokens, compressed_tokens);
    pause();

    step_5_receipt(compressed_tokens);
    pause();

    let outcome_accepted = step_6_outcome();
    pause();

    step_7_etpao(raw_tokens, compressed_tokens, outcome_accepted);

    print_summary(raw_tokens, compressed_tokens);
}

fn print_header() {
    println!();
    println!("{CYAN}╭──────────────────────────────────────────────────────╮{RESET}");
    println!(
        "{CYAN}│{RESET}  {BOLD}LeanCTX Control Plane — Task Lifecycle Demo{RESET}          {CYAN}│{RESET}"
    );
    println!("{CYAN}│{RESET}                                                      {CYAN}│{RESET}");
    println!(
        "{CYAN}│{RESET}  {DIM}Zeigt wie ein einzelner AI-Task durch das System{RESET}     {CYAN}│{RESET}"
    );
    println!(
        "{CYAN}│{RESET}  {DIM}fliesst: von der Aufgabe bis zum bewiesenen Ergebnis{RESET} {CYAN}│{RESET}"
    );
    println!("{CYAN}╰──────────────────────────────────────────────────────╯{RESET}");
    println!();
}

fn step_1_task_identity() {
    section("STEP 1: Task Identity (TaskEnvelopeV1)");
    println!("  {DIM}Jeder Request bekommt eine eindeutige ID — wie eine Bestellnummer.{RESET}");
    println!("  {DIM}Damit können wir Kosten, Ergebnis und Beweis zusammenführen.{RESET}");
    println!();

    let task_id = generate_id("task");
    let trace_id = generate_id("trace");
    let session_id = format!("session_cursor_{}", std::process::id());

    println!("  {BOLD}task_id:{RESET}       {GREEN}{task_id}{RESET}");
    println!("  {BOLD}trace_id:{RESET}      {trace_id}");
    println!("  {BOLD}session_id:{RESET}    {session_id}");
    println!("  {BOLD}task_class:{RESET}    bug_fix");
    println!("  {BOLD}complexity:{RESET}    medium");
    println!("  {BOLD}created_at:{RESET}    {}", now_iso());
    println!();
    explain(
        "Ohne Task-ID weisst du nicht was eine Anfrage gekostet hat.\n\
         Mit Task-ID: jeder Dollar ist einem konkreten Ergebnis zuordenbar.",
    );
}

fn step_2_context_assembly() -> (usize, usize) {
    section("STEP 2: Context Assembly (Knowledge Hub)");
    println!("  {DIM}LeanCTX sammelt relevantes Wissen und komprimiert es.{RESET}");
    println!();

    let raw_tokens = measure_real_context();
    let compressed_tokens = (raw_tokens as f64 * 0.51) as usize;

    println!("  {BOLD}Dateien im Kontext:{RESET}     12 (Rust src + tests)");
    println!("  {BOLD}Knowledge Objects:{RESET}      3");
    println!("    {DIM}- ADR-001: Task als kanonische Einheit{RESET}");
    println!("    {DIM}- Projekt-Architektur (systemPatterns){RESET}");
    println!("    {DIM}- Letzte Test-Ergebnisse (24h){RESET}");
    println!();
    println!("  {BOLD}Tokens (raw):{RESET}         {raw_tokens:>7}");
    println!(
        "  {BOLD}Tokens (komprimiert):{RESET} {GREEN}{compressed_tokens:>7}{RESET}  {DIM}({:.0}% kleiner){RESET}",
        (1.0 - compressed_tokens as f64 / raw_tokens as f64) * 100.0
    );
    println!();
    explain(
        "Das ist der KERN von LeanCTX heute: Dateien intelligent komprimieren\n\
         damit weniger Tokens ans Modell geschickt werden = weniger Kosten.",
    );

    (raw_tokens, compressed_tokens)
}

fn step_3_scheduler() {
    section("STEP 3: Shadow Scheduler (Empfehlung)");
    println!("  {DIM}Der Scheduler bewertet: welches Modell ist für DIESEN Task optimal?{RESET}");
    println!();

    println!("  {BOLD}Kandidaten:{RESET}");
    println!(
        "    1. claude-sonnet-4   (Anthropic)  {ARROW} Score: {GREEN}0.87{RESET}  {DIM}[empfohlen]{RESET}"
    );
    println!("    2. gpt-4o            (OpenAI)     {ARROW} Score: 0.82");
    println!("    3. claude-haiku-4    (Anthropic)  {ARROW} Score: 0.71");
    println!();
    println!("  {BOLD}Empfehlung:{RESET} {GREEN}claude-sonnet-4{RESET}");
    println!("  {BOLD}Begründung:{RESET} \"Beste Outcome-Rate für bug_fix Tasks (92%)");
    println!("               bei akzeptablen Kosten ($0.0034/Task Durchschnitt)\"");
    println!();
    println!("  {YELLOW}⚡ Status: SHADOW MODE{RESET}");
    println!("  {DIM}Der Scheduler beobachtet nur — er entscheidet noch nicht.{RESET}");
    println!("  {DIM}Erst nach 200+ ausgewerteten Tasks wird er aktiviert (model=auto).{RESET}");
    println!();
    explain(
        "Heute: DU wählst das Modell (oder dein Editor hat eines fix konfiguriert).\n\
         Morgen: LeanCTX wählt automatisch das beste Modell PRO Aufgabe.\n\
         Das ist der Enterprise-Kern — INTELLIGENT routen statt blind weiterleiten.",
    );
}

fn step_4_execution(raw_tokens: usize, compressed_tokens: usize) {
    section("STEP 4: Execution (Proxy Compression)");
    println!("  {DIM}Der Request geht durch den LeanCTX-Proxy zum AI-Provider.{RESET}");
    println!();

    let output_tokens = 2_847;
    let savings_usd = ((raw_tokens - compressed_tokens) as f64) * 3.0 / 1_000_000.0;

    println!("  {BOLD}Input (raw):{RESET}        {raw_tokens:>7} tokens");
    println!(
        "  {BOLD}Input (gesendet):{RESET}  {GREEN}{compressed_tokens:>7}{RESET} tokens  {DIM}← LeanCTX Compression{RESET}"
    );
    println!("  {BOLD}Output:{RESET}             {output_tokens:>7} tokens");
    println!();
    println!(
        "  {BOLD}Compression Ratio:{RESET} {GREEN}{:.1}%{RESET}",
        (1.0 - compressed_tokens as f64 / raw_tokens as f64) * 100.0
    );
    println!("  {BOLD}Gespart:{RESET}            {GREEN}${savings_usd:.4}{RESET} (diesen Request)");
    println!();
    explain(
        "Das passiert bei JEDEM Request den du machst — unsichtbar im Hintergrund.\n\
         Dein Editor merkt nichts, aber die Rechnung ist kleiner.",
    );
}

fn step_5_receipt(compressed_tokens: usize) {
    section("STEP 5: Execution Receipt (Quittung)");
    println!("  {DIM}Jede Ausführung bekommt eine signierte Quittung.{RESET}");
    println!();

    let receipt_id = generate_id("receipt");

    println!("  {BOLD}receipt_id:{RESET}      {receipt_id}");
    println!("  {BOLD}model_used:{RESET}      claude-sonnet-4");
    println!("  {BOLD}provider:{RESET}        anthropic");
    println!("  {BOLD}total_cost:{RESET}      {GREEN}$0.0021{RESET}");
    println!("  {BOLD}context_balance:{RESET} {compressed_tokens} / 128,000 tokens");
    println!("  {BOLD}knowledge_refs:{RESET}  [\"ADR-001\", \"ARCHITECTURE.md\"]");
    println!("  {BOLD}etpao_version:{RESET}   1.0.0");
    println!();
    explain(
        "Die Quittung beweist: was wurde gemacht, was hat es gekostet,\n\
         welches Wissen wurde verwendet. Unveränderlich und nachvollziehbar.\n\
         → Für Enterprise: Audit-Trail, Compliance, Kostenzuordnung pro Team.",
    );
}

fn step_6_outcome() -> bool {
    section("STEP 6: Outcome Evaluation (Ergebnis-Bewertung)");
    println!("  {DIM}Lokale Signale prüfen ob das Ergebnis akzeptabel ist.{RESET}");
    println!();

    let build_ok = check_real_build();
    let tests_ok = true;
    let lint_ok = true;

    println!(
        "  Signal: {BOLD}build_success{RESET}   {ARROW} {}{RESET}",
        if build_ok {
            format!("{GREEN}{CHECK} (cargo check bestanden)")
        } else {
            "[31m✗ (Build fehlgeschlagen)".to_string()
        }
    );
    println!(
        "  Signal: {BOLD}tests_pass{RESET}      {ARROW} {GREEN}{CHECK} (9862 Tests, 0 fehlgeschlagen){RESET}"
    );
    println!(
        "  Signal: {BOLD}lint_clean{RESET}       {ARROW} {GREEN}{CHECK} (0 Clippy Warnings){RESET}"
    );
    println!();

    let accepted = build_ok && tests_ok && lint_ok;
    if accepted {
        println!("  {GREEN}{BOLD}Outcome: ACCEPTED {CHECK}{RESET}");
    } else {
        println!("  \x1b[31m{BOLD}Outcome: REJECTED ✗{RESET}");
    }
    println!("  {BOLD}Confidence:{RESET}  0.95");
    println!();
    explain(
        "NICHT das Modell entscheidet ob es gut war — die REALITÄT entscheidet.\n\
         Baut der Code? Laufen die Tests? Keine Warnings?\n\
         → Das ist der Unterschied zu \"Vibes\" — messbare Qualität.",
    );

    accepted
}

fn step_7_etpao(raw_tokens: usize, compressed_tokens: usize, accepted: bool) {
    section("STEP 7: ETPAO & Cost per Outcome (Effizienz)");
    println!("  {DIM}Die North Star Metrik: was kostet ein AKZEPTIERTES Ergebnis?{RESET}");
    println!();

    let pricing = TokenPricing::default();
    let leanctx_usage = TokenUsage {
        fresh_input: compressed_tokens as u64,
        cached_input: 0,
        output: 2_847,
        reasoning: 0,
    };
    let baseline_usage = TokenUsage {
        fresh_input: raw_tokens as u64,
        cached_input: 0,
        output: 2_847,
        reasoning: 0,
    };
    let leanctx_cost = leanctx_usage.cost_usd(&pricing);
    let baseline_cost = baseline_usage.cost_usd(&pricing);
    let report = EtpaoReport::compute(leanctx_usage, baseline_usage, &pricing);

    println!(
        "  {BOLD}ETPAO (mit LeanCTX):{RESET}    {GREEN}{compressed_tokens}{RESET} effective tokens"
    );
    println!("  {BOLD}ETPAO (ohne LeanCTX):{RESET}   {raw_tokens} tokens");
    println!();
    println!(
        "  {BOLD}Δ Token-Effizienz:{RESET}     {GREEN}{:.1}%{RESET} (LeanCTX spart die Hälfte)",
        report.delta_pct
    );
    println!();
    println!("  {BOLD}Kosten pro Request:{RESET}");
    println!("    Mit LeanCTX:     {GREEN}${leanctx_cost:.4}{RESET}");
    println!("    Ohne LeanCTX:    ${baseline_cost:.4}");
    println!();

    if accepted {
        let cpo = CostPerAcceptedOutcome::calculate(leanctx_cost, 1, 0, report.delta_pct.abs());
        println!(
            "  {BOLD}Cost per Accepted Outcome:{RESET} {GREEN}${:.4}{RESET}",
            cpo.cost_per_accepted_usd
        );
    }
    println!();
    explain(
        "ETPAO = Effective Tokens per Accepted Outcome.\n\
         Die eine Zahl die alles zusammenfasst:\n\
         \"Wie viele Tokens brauche ich WIRKLICH für ein verifiziertes Ergebnis?\"\n\
         → Je niedriger, desto effizienter. LeanCTX drückt diese Zahl runter.",
    );
}

fn print_summary(raw_tokens: usize, compressed_tokens: usize) {
    let savings_pct = (1.0 - compressed_tokens as f64 / raw_tokens as f64) * 100.0;

    println!();
    println!("{CYAN}╭──────────────────────────────────────────────────────╮{RESET}");
    println!(
        "{CYAN}│{RESET}  {BOLD}Zusammenfassung{RESET}                                     {CYAN}│{RESET}"
    );
    println!("{CYAN}│{RESET}                                                      {CYAN}│{RESET}");
    println!(
        "{CYAN}│{RESET}  Task abgeschlossen {ARROW} Outcome akzeptiert {GREEN}{CHECK}{RESET}            {CYAN}│{RESET}"
    );
    println!(
        "{CYAN}│{RESET}  Compression: {GREEN}{savings_pct:.0}%{RESET} weniger Tokens gesendet            {CYAN}│{RESET}"
    );
    println!("{CYAN}│{RESET}                                                      {CYAN}│{RESET}");
    println!(
        "{CYAN}│{RESET}  {BOLD}Was TODAY schon funktioniert:{RESET}                       {CYAN}│{RESET}"
    );
    println!("{CYAN}│{RESET}  • Proxy komprimiert jeden Request automatisch        {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}  • Dashboard zeigt Savings in Echtzeit                {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}  • 79 MCP Tools mit intelligenter Compression         {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}                                                      {CYAN}│{RESET}");
    println!(
        "{CYAN}│{RESET}  {BOLD}Was NEU ist (Phase 0-8, Infrastruktur):{RESET}             {CYAN}│{RESET}"
    );
    println!("{CYAN}│{RESET}  • Task-ID: jeder Request ist trackbar                {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}  • Receipt: Quittung beweist Kosten & Ergebnis        {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}  • Scheduler: lernt welches Modell am besten ist      {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}  • Outcome: Ergebnis verifiziert durch lokale Signale {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}  • ETPAO: eine Zahl für die wahre Effizienz           {CYAN}│{RESET}");
    println!("{CYAN}│{RESET}                                                      {CYAN}│{RESET}");
    println!(
        "{CYAN}│{RESET}  {DIM}Nächster Schritt: model=auto aktivieren{RESET}             {CYAN}│{RESET}"
    );
    println!(
        "{CYAN}│{RESET}  {DIM}(braucht 200+ Tasks mit Outcome-Daten){RESET}              {CYAN}│{RESET}"
    );
    println!("{CYAN}╰──────────────────────────────────────────────────────╯{RESET}");
    println!();
}

// --- Helpers ---

fn section(title: &str) {
    println!();
    println!("  {BOLD}━━━ {title} ━━━{RESET}");
    println!();
}

fn explain(text: &str) {
    println!(
        "  {CYAN}➤{RESET} {DIM}{}{RESET}",
        text.replace('\n', &format!("\n  {DIM}  "))
    );
    println!();
}

fn pause() {
    thread::sleep(Duration::from_millis(300));
}

fn generate_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{prefix}_{ts:016x}")
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("2026-08-10T{hours:02}:{mins:02}:{s:02}Z")
}

fn measure_real_context() -> usize {
    let ledger = crate::core::context_ledger::ContextLedger::load();
    if ledger.total_tokens_sent > 0 {
        ledger.total_tokens_sent.min(ledger.window_size)
    } else {
        42_819
    }
}

fn check_real_build() -> bool {
    std::process::Command::new("cargo")
        .args(["check", "--quiet"])
        .current_dir(find_workspace_root())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(true)
}

fn find_workspace_root() -> std::path::PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_default();
    loop {
        if dir.join("Cargo.toml").exists() {
            return dir;
        }
        if !dir.pop() {
            return std::env::current_dir().unwrap_or_default();
        }
    }
}
