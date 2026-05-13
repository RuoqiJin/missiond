# Long Image Service Vercel Deployment Experience

## Context

Project: `/Users/jinchen/Projects/long-image-service`

Repository: `https://github.com/RuoqiJin/long-image-service`

Vercel project: `rickyjim626s-projects/long-image-service`

Production URL: `https://long-image-service.vercel.app`

## Deployment Steps That Worked

```bash
gh repo create RuoqiJin/long-image-service --private --source=. --remote=origin --push
npm i -g vercel@latest --ignore-scripts
vercel --prod --yes
curl -s -L -o /tmp/long-image-service-vercel.html -w '%{http_code} %{size_download} %{url_effective}\n' https://long-image-service.vercel.app/
```

## Lessons For MissionD

- Vercel operation should be a first-class deploy-center / MissionD ops capability, not ad-hoc CLI memory.
- `vercel --prod --yes` can create/link a project and deploy if the CLI is logged in.
- Vercel CLI version matters: `42.3.0` failed against current Vercel API; `53.4.0` worked.
- `.vercel/` must be ignored and not committed.
- Static Vercel deployment is not equivalent to full service production. If the product needs history, billing, or render API, MissionD should require one of:
  - Vercel storage/serverless adapter; or
  - a separate global API deployment configured through `VITE_API_BASE_URL`.

## Suggested MissionD Follow-Up

- Add a `vercel-deployment` operations workflow with phases: auth check, CLI version check, project link, env check, prod deploy, smoke URL, capture alias.
- Add a deployment completeness classifier: `frontend-only`, `frontend-with-api`, `full-service`.
- Store Vercel project id / alias / deployment url in project runtime evidence, while deploy-center remains the production deployment authority.
