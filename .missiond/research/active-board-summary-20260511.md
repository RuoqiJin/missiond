# Active Board Summary - 2026-05-11

Active means status is not `done`, `skipped`, or `completed`.

## Counts

- Total active rows: 52.
- Blocked: 10.
- Failed: 3.
- Open: 39.
- MissionD-owned active rows: 5.

## MissionD Infrastructure

- `fe506dd5-0f99-40a9-89e0-0fd4d5d69600` Search control-plane: Grok AI-search + Tavily/Bocha deterministic adapters.
- `9e71cf4b-ae65-4fe1-99c5-f0c652477267` PTY anomaly recovery: frozen Thinking/ToolRunning interrupt/restart guard.
- `6a9960a5-580e-4aaf-a93f-6afbe9d42f07` PTY integration tests for upstream CLI changes.
- `a37f5ddc-2fe2-4453-a775-059acfda0eb7` Systematic MissionD KB: layered knowledge and retrieval governance.
- `3b217aee-0b47-4ac6-bc28-4c466793c9dd` Memory worker compact resilience.

## XJP / App SSOT And Backfill

- `4043434b-75e7-4102-a5c0-af9ccda935cb` xiaojinpro-backend M6.
- `20e04cee-3eb1-4540-ae57-20ceae291f0c` payments M6.
- `cceb15fc-ccf7-4004-9a4d-47f255a64d77` asr M6.
- `1c1cb4a9-8c22-4cbf-9e41-680b904d16e6` timeline M6.
- `50d80ae8-1e82-498c-97be-1c356e14e4bc` failed INFRA_M6 shard A1.
- `892390fd-48a5-4253-adff-5d01a7913985` failed router M6 shard.
- `2db487ff-756d-48ad-ad07-911a515bd49f` failed payments M6 shard.
- `80e7b1ee-fe49-4f40-a643-3e004b400982` xiaojinpro-frontend commit `93e7630f` backfill.
- `a02279af-426d-421b-9786-0630b70125d6` same commit coverage backfill.
- `da271ca9-297d-4c4a-bcc2-b0af01a0bef3` xiaojinpro-backend commit `ac37a16` backfill.
- `04a79f1d-9031-46d7-b097-5a3bc44b717c` pcea-video-vault commit `8aeac84` backfill.

## Infra / Ops

- `d53ad30f-3aa2-446d-bd3e-6cf065754e1f` Synology 1819+ disk data management plan.
- `bc0297f1-9616-4570-a5b2-1c3851ed5728` PCEA self-hosted VOD cost reduction.
- `d5e21000-3bcc-4ed3-a465-b9049e67bc67` sync Feishu/Baidu domains to private-cloud dnsmasq.
- `a9345873-cc60-4782-916a-9dacdb9b09d6` test Claude Code on private cloud.
- `496805e3-3e38-43c5-8e8a-c98dcaed6ca3` Claw.cloud Pro+ renewal reminder.
- `88476117-7ab9-4db8-9796-4c0e87060a17` Aliyun OSS package renewal.
- `5dc84720-52b5-4b3a-be47-67608cf2cc2d` Aliyun ECS renewal.
- `fdc53fca-0b84-468e-833b-b01637a03a3c` BWG/VPS renewal.

## Product / Application Work

- `c16a2e2c-c7ca-4775-86bd-1ae2d509f627` xiaojinpro.top Claude Code remote control panel.
- `636e46ce-38f7-403f-9266-c45a124a78da` CutHub AI Canvas.
- `3dc19c07-53db-48f1-bc6d-ca5843a03b8b` Earth-Journey Node.js to Rust migration.
- `baa6f8b5-941c-4b60-97d5-a1e62d1d22a9` earth-journey general map microservice.
- `e22a40ea-e48e-4829-81aa-18f1be7bdb60` PCEA paper-edit permission flow.
- `e1c10d11-7e37-4e4b-8a24-1e500660859f` PCEA paper-edit video link callback.
- `b52441f7-6576-4abf-b318-8695b4756992` CutHub paper-edit feature.
- `9c2ff984-5cc8-414d-8fc6-6ae843dc506a` CutHub video link + prompt pool.
- `4842b9c9-8f02-4e88-9b35-28f520cea041` PCEA video organization + Tencent VOD playback.
- `9790a390-3c31-42fe-8cc1-2d5a7dd5f985` PCEA video to CutHub to self-hosted VOD flow.
- `21c37cfb-763d-4696-8e0d-af8879ca0dfb` PCEA Lobster Pond.
- `011c767d-5ffa-4a80-a146-207409d293f5` Baidu Netdisk online preview.
- `b092d79b-b8cf-41b4-bee6-ef87430db8bb` Transfer page XiaojinPro QR login.
- `b3c3c00f-1a26-4b41-897a-5be81a15a850` monitoring center page.
- `560db266-86c3-48c4-a550-dc2fceb1b15a` Neural Codegen Phase 1.

## Other Historical Open Items

- `9ab0955b-e818-4f0a-ac20-8da273306d38` research: space elevator material science.
- `5590c611-411e-418d-86ef-43e14b4be9d8` earth-journey camera pause bug.
- `e62ec17d-83d0-4c3c-b45c-956245370c52` earth-journey Melbourne marker bug.
- `102579e2-4ef5-46d4-a596-66aa6e81dcef` iOS voice input issue.
- `6c6744f9-161d-4e43-98c9-5d94a163a2aa` iOS voice input no UI feedback.
- `f1a581a8-80f8-4655-9b5e-7ba68be9dc60` yt-dlp source expansion.
- `95ac10a9-010b-45da-b984-c321fdd4befd` editing deliverable to iCloud to WeChat automation.

## Duplicate-Looking Workflow Automation Rows

Five old open rows describe the same stdin/backoff rule for WebSocket/SDK/third-party CLI subprocesses:

- `3fca49fd-f0f6-4c51-be3b-0a9a67e3eb2e`
- `0a289ce1-c8a7-41b7-b265-de60260ac632`
- `fe8355e4-d824-4ccf-a0eb-47a68db22262`
- `6fa69cc4-4484-4258-b0c8-3ba1588064ef`
- `9b78511d-80e8-4182-9413-891b98951f5f`

These should be merged or closed against the existing MissionD process/slot lifecycle rules in a follow-up cleanup pass.
