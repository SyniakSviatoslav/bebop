# FABLE DEEP RESEARCH — tools/protocols reconnaissance + bebop2 multi-param surface

Two independent fable-style deep-research agents, run in parallel (deleg_f9898a4a = tooling
reconnaissance; deleg_e7b7714e = bebop2 continuous multi-parameter surface). Research only;
honest-boundary discipline enforced (WormGPT refused; git-spoofing called provenance red-line).

═══════════════════════════════════════════════════════════════════
PART A — 35 ITEMS: WHAT THEY ARE / VALUE / DUAL-USE (deleg_f9898a4a)
═══════════════════════════════════════════════════════════════════

VERIFIED AGAINST PRIMARY SOURCES (official repos, arXiv 2310.10688, RFCs, docs):

1. I2P garlic routing — P2P anon overlay; garlic = multiple msgs bundled in one encrypted tunnel;
   UNIDIRECTIONAL tunnels; DHT peer selection; built for eepsites (hidden svc), no global dir.
   Differs from Tor (bidirectional circuits, dir-authority, clearnet-optimized). Anonymity set =
   all I2P routers. Strong for hidden svc; smaller set than Tor. Over-claim "more anon than Tor"
   is FALSE generally.
2. secret-patterns-db (= mazen160/secrets-patterns-db) — ~1600+ regex secret-detection patterns.
   High value as a gitleaks/trufflehog FEED for pre-commit/CI. Risk: regex-only ⇒ false positives;
   never a standalone gate. Not malicious.
3. Openspace — TWO things: (a) NASA OpenSpace astro/universe viz (likely the intended one); (b)
   HKUDS/OpenSpace AI-agent framework. No security angle for (a).
4. Openweb-ui (= open-webui) — self-hosted LLM chat, offline, Ollama/OpenAI backends. SECURITY:
   Tools/Functions execute user Python on server BY DESIGN; CVE class (command-injection, e.g.
   tracked CVE-2026-0765). Bind localhost + enable auth; never expose untrusted.
5. google-research/timesfm — Time-FM, decoder-only PATCHED attention (arXiv 2310.10688 Das/Kong/
   Sen/Zhou). ~100B time-points pretrain; zero/few-shot; timesfm-1.0-200m = Apache-2.0. HONEST:
   zero-shot "comes close to" per-dataset SOTA, NOT uniformly better; on SHORT series ARIMA/Prophet/
   seasonal-naive often still win. Don't overclaim.
6. Asgeirtj/system_prompt_leaks — archive of others' LLM system prompts. No tool; privacy/ToS
   concern. Not a capability.
7. claude-video — UNVERIFIED/ambiguous. No canonical Anthropic product; likely a third-party
   wrapper or cookbook example. Marked unknown, not fabricated.
8. pocket-tts (= kyutai-labs/pocket-tts, NOT HF/Xenova-branded) — ~100M-param TTS, real-time on
   CPU, voice-clone from short samples, ~200ms latency. Genuine good small-footprint TTS.
   Dual-use: voice-clone deepfake potential.
9. justvugg/colibri — pure-C zero-dep inference engine running GLM-5.2 (744B MoE) on ~25GB RAM
   by streaming experts from disk. ML-systems engineering, not RAG. Benign model runner.
10. gitghost — PREMISE CORRECTION: real repos (vadimdemedes/gitghost = publish to Ghost via git;
    imliam/gitghost = mirror commit history between repos) are BENIGN; neither spoofs authors.
    Author-spoofing is native `git commit --author`; the ACT is the provenance red-line, not this tool.
11. lebesgue integral — genuine measure-theory integral (sup over simple functions). Applies to
    E[X]=∫x dP, L^p spaces, Fourier. Foundational math, not a tool.
12. Cariddi (= edoardottt/cariddi) — fast Go crawler: endpoints/secrets/API-keys/tokens in JS.
    Dual-use (bug-bounty recon vs attacker key-hunting).
13. ip-tracer / ip tracker — IP geolocation CLIs (ip-api.com). Accuracy = city-level, often wrong
    for mobile/VPN/cloud. Dual-use.
14. chafa (= hpjansson/chafa) — image/GIF → ANSI/Unicode terminal art. Harmless cosmetic.
15. wormgpt — MALICIOUS. Uncensored LLM for BEC/phishing/malware (Palo Alto Unit42, Talos, Rapid7).
    REFUSED operational detail. Threat-indicator only; adversary, never a tool.
16. neovim — Lua-extensible Vim. Top dev editor. None.
17. traceroute — TTL-limited diagnostic. Genuine.
18. tik-osint — TikTok OSINT tooling (Omicron166/TikTok-OSINT, HackUnderway/TokIntel). Dual-use.
19. blackbird-osint (= p1ngul1n0/blackbird) — 600+ platform username/email search, AI profiling.
    Dual-use (doxxing).
20. webinfo (= Anon4You/webinfo) — Termux script: WHOIS/DNS/GeoIP/HTTP-header/port-scan menu. Recon.
21. onefetch (= o2sh/onefetch) — Rust git repo info viz. Genuine dev tool.
22. aliens-eye (= arxhr007/Aliens_eye) — AI username scanner 800–840+ platforms. Dual-use.
23. dufs (= sigoden/dufs) — Rust file server: upload/WebDAV/search, optional basic auth. Risk:
    --allow-all/no-auth + exposure = open server. Bind localhost or set auth.
24. lynx — text-mode browser. Genuine low-footprint.
25. sentinel octopus — UNVERIFIED canonical tool; appears a YouTube creator project (SentinelProxy/
    AttackChain). Not infrastructure. Distinct from SentinelOne EDR.
26. nmap — industry-standard scanner. Dual-use; authorize before scanning.
27. netcat (nc) — networking Swiss-army. Dual-use: bind-shell = attacker staple.
28. masscan — TCP SYN 10M-pps scanner. Dual-use; aggressive.
29. web-tech scanning — Wappalyzer / whatweb (OWASP-recommended) / wad. Fingerprinting. Dual-use.
30. rustscan (= bee-san/RustScan) — 65k ports/~3s, pipes to nmap. Dual-use.
31. naabu (= projectdiscovery/naabu) — SYN port scanner, PD suite. Dual-use.
32. dns-scanning — dnsx (PD) / amass (OWASP) / subfinder (PD) / dnsrecon. Recon. Dual-use.
33. discovery workflow — passive (CT logs, passive DNS, subfinder/amass) → active (naabu/rustscan/
    masscan) → fingerprint (nmap) → vuln (nuclei/httpx). Authorize first; escalates intrusiveness.
34. spiderfoot (= smicallef/spiderfoot) — 100+ module OSINT automation, web UI. Attack-surface
    mapping / threat intel. Dual-use.
35. termux-localhost / termux — Android terminal + pkg. Lets you run recon from mobile; same
    dual-use risk; mobile IP ≠ anonymity.

DEFENSIVE AGENT RECOMMENDATION (bebop-style):
MAY legitimately use (own hardening / local / authorized): onefetch; rustscan/naabu/nmap ONLY vs
OWN assets; lynx/dufs(localhost+auth); chafa; neovim; traceroute (own infra); secrets-patterns-db as
gitleaks feed for OWN repos; SpiderFoot vs OWN domains; TimesFM/pocket-tts/colibri/I2P/OpenWebUI
(localhost+auth); OpenSpace (viz).
MUST NOT weaponize: WormGPT (refused, threat-indicator); Cariddi/masscan/nc-bind-shell/blackbird/
aliens-eye/tik-osint/webinfo/ip-tracer/Sentinel-Octopus ONLY on own assets or w/ written auth;
git author-spoofing = provenance red-line (act, not gitghost tool).

═══════════════════════════════════════════════════════════════════
PART B — bebop2 NET-NEW CONTINUOUS MULTI-PARAM SURFACE (deleg_e7b7714e)
═══════════════════════════════════════════════════════════════════

CORRECT OBJECT: U: G→ℝ^k multichannel field, column-major U[i*n+c] (matches
LaplacianSpectrum.modes, field.rs:29). Each channel solves Fick/graph-heat ∂_t u_c = −L u_c
(L = D−A, field.rs:77; columns sum to zero ⇒ mass conserved per channel: d/dt Σu_c = 0).
Coupled option: ∂_t u_c = −Σ_d C_cd·L u_d (matrix diffusion).

CONTINUOUS/ONLINE UPDATE: edge-weight change perturbs L by symmetric rank-2 ΔL = Δw·(e_i−e_j)(e_i−e_j)^T.
Two bounded strategies: (a) LAZY eigen-decomp — keep (eigenvalues, modes) from jacobi_eigen
(field.rs:257); recompute only when ‖ΔL‖₂ exceeds threshold (Davis–Kahan gap bounds the slip);
(b) INCREMENTAL eigen update via resolvent R(z)=(I−zΛ)⁻¹ (mirrors kalman.rs:200 covariance
recurrence). Both O(bounded), no full re-diagonalization every step.

SYMPLECTIC FOR WAVE CHANNELS ONLY: any hyperbolic channel ∂_t²w_c = −c² L w_c MUST use
velocity-Verlet (Verlet 1967; Hairer–Lubich–Wanner), NOT explicit Euler (injects energy).
v_{n+½}=v_n+dt/2·(−L w_n); w_{n+1}=w_n+dt·v_{n+½}; v_{n+1}=v_{n+½}+dt/2·(−L w_{n+1}).
E=Σ½v²+½wᵀLw conserved O(dt²). NOTE: field_physics.rs:334 step_wave uses forward Euler + damping
(field_physics.rs:53 WAVE_DAMP) — fine for DISSIPATIVE wave (E decays, wave_energy :301 = Lyapunov),
but a NON-damped wave KAT must use Verlet, not step_wave.

FALSIFIABLE KATs (each RED when wrong — Verified-by-Math):
- multi_diffusion_conserves_mass — propagate_spectral/active_diffuse (field.rs:136,168) on k chans,
  no source; |ΣΣu_c(t) − ΣΣu_c(0)| < 1e-3 on connected graph (λ₀=0, spectrum_has_zero_mode :361).
- edge_add_perturbation_bound — add edge, recompute jacobi_eigen (:257); |λ_i^new−λ_i^old| ≤ ‖ΔL‖₂
  (Weyl C=1; Davis–Kahan companion). RED if exceeded.
- wave_verlet_energy_constant — Verlet E-drift < ε (1e-6); SAME as explicit Euler MUST drift > ε
  (proves integrator choice is load-bearing, mirrors propagator_red_breaks_on_coeff_change :276).
- field_gate red-line — redline_task_is_vetoed (:263) stays RED; channels MUST NOT weaken
  field_gate_verdict (:127, TOLERANCE 0.10).

INTEGRATION PLAN (file:line, backward-compatible — old single-channel KATs stay GREEN):
- chebyshev.rs:111 spectral_propagate — loop channels c in 0..k, scatter; k=1 = old API.
- field.rs:136 propagate_spectral — add channel: usize default 0.
- field.rs:174 field_kalman — per-channel kalman1d_step (analytics.rs:7) on each channel's mass
  series; loop_health (:234) consumes smoothed series unchanged.
- stabilizer.rs:114 stabilize_step — energy Σ_c E_c; lyapunov_derivative (:42) takes multi-channel
  total; field_physics.rs:301 wave_energy already sums per-body V-tensors ⇒ multi-channel = existing
  V-tensor, dimension preserved.
- multipilot.rs:45/:113 — fan each channel as distinct field context; aggregate; keep field_gate
  at :78; field_override_blocks (:169) stays RED.
Old KATs (bridge_cost_conserves_mass :417, kalman_converges_to_constant_signal :332,
propgator_matches_old_oracle_mass chebyshev.rs:205) untouched ⇒ GREEN by construction.

HONEST VERDICT (multi-param surface):
LEGIT (code as physics): Fick diffusion per channel = load-balancing/demand-spreading (real);
spectral λ₂ Fiedler = fragility/rupture (real); spherical-harmonic channels for angular coverage
(geometry_field.rs:220,:238, real); co-located scalars = legitimate tensor (field_physics V-tensor,
wavefield.rs:73 ConnEdge + LinkKind).
POETRY (must NOT simulate as physics; relabel as scalar channels at most): Emden "demand black
holes" (nonlinear sink, not self-gravitating collapse); redshift "trust coefficient" (staleness/TTL
decay scalar); vorticity "courier loops" (topological cycle detection, not fluid curl); Noether/Fock/
Catalan = narrative only.

RISK / RED-LINE (bounded continuous update):
- Cap channels k ≤ MAX_CHANNELS; cap graph order n (jacobi_eigen is O(n³) → cap n or power-iteration
  for large graphs).
- Bound edge-change queue; apply ΔL lazily (strategy a).
- memory.rs:60 tick hash-mod-7 eviction already bounds LivingMemory (NOT Padovan — unimplemented).
- cgroups memory.max / pids.max per prior research.
- **ACTIONABLE FINDING**: mcp.rs:215 call_tool has NO length guard (search found none). REQUIRED:
  `if args.to_string().len() > MAX_ARG_LEN { return Err(...) }` before dispatch; cap n/batches in
  run_multipilot (:45)/batch_dispatch (:113). Fail-closed.

NET: multi-channel surface is a strict, correct generalization of the existing Fick/Laplacian engine;
Verlet mandatory for wave channels; Davis–Kahan/Weyl make online L-updates falsifiable; Emden/
redshift/vorticity stay labeled scalar channels, never simulated astrophysics/fluids.
