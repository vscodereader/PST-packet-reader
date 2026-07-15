#!/usr/bin/env bash
# 원타임 GCP 셋업: Cloud Run 배포용 Artifact Registry + 배포 서비스계정 + GitHub WIF + Secret Manager.
# (bambi-app/scripts/setup-gcp-cloud-run.sh 를 pstmacro 용으로 적응 — 시크릿 3종, 단일 서비스, 마이그레이션 권한 없음.)
#
# 사전 조건: GCP 콘솔의 Cloud Shell(또는 gcloud 설치된 셸)에서 로그인 완료, pstmacro-gcp 프로젝트 존재.
# 실행:  bash scripts/setup-gcp-cloud-run.sh
set -euo pipefail

PROJECT_ID="${PROJECT_ID:-pstmacro-gcp}"
REGION="${REGION:-asia-northeast3}"
REPOSITORY="${REPOSITORY:-pstmacro}"
GITHUB_REPO="${GITHUB_REPO:-beyondsoft-kr/pstmacro}"
SERVICE="${SERVICE:-pstmacro-server}"
SA_NAME="github-deployer"
POOL_ID="github-pool"
PROVIDER_ID="github-provider"

echo "▶ 프로젝트: ${PROJECT_ID} / 리전: ${REGION} / GitHub: ${GITHUB_REPO}"
gcloud config set project "${PROJECT_ID}" >/dev/null

echo "▶ 1/6 API 활성화"
gcloud services enable \
	run.googleapis.com \
	artifactregistry.googleapis.com \
	iamcredentials.googleapis.com \
	sts.googleapis.com \
	secretmanager.googleapis.com

echo "▶ 2/6 Artifact Registry(docker) 저장소: ${REPOSITORY}"
if ! gcloud artifacts repositories describe "${REPOSITORY}" --location="${REGION}" >/dev/null 2>&1; then
	gcloud artifacts repositories create "${REPOSITORY}" \
		--repository-format=docker \
		--location="${REGION}" \
		--description="pstmacro container images"
fi

echo "▶ 3/6 배포용 서비스계정: ${SA_NAME}"
SA_EMAIL="${SA_NAME}@${PROJECT_ID}.iam.gserviceaccount.com"
if ! gcloud iam service-accounts describe "${SA_EMAIL}" >/dev/null 2>&1; then
	gcloud iam service-accounts create "${SA_NAME}" --display-name="GitHub Actions deployer"
fi
gcloud projects add-iam-policy-binding "${PROJECT_ID}" \
	--member="serviceAccount:${SA_EMAIL}" --role="roles/run.admin" --condition=None >/dev/null
gcloud projects add-iam-policy-binding "${PROJECT_ID}" \
	--member="serviceAccount:${SA_EMAIL}" --role="roles/artifactregistry.writer" --condition=None >/dev/null

PROJECT_NUMBER="$(gcloud projects describe "${PROJECT_ID}" --format='value(projectNumber)')"
RUNTIME_SA="${PROJECT_NUMBER}-compute@developer.gserviceaccount.com"
# Cloud Run 서비스가 기본 컴퓨트 SA로 실행되므로, 배포자가 그 SA를 "사용"할 권한 필요.
gcloud iam service-accounts add-iam-policy-binding "${RUNTIME_SA}" \
	--member="serviceAccount:${SA_EMAIL}" --role="roles/iam.serviceAccountUser" >/dev/null

echo "▶ 4/6 Workload Identity Federation (키리스 GitHub 인증)"
if ! gcloud iam workload-identity-pools describe "${POOL_ID}" --location=global >/dev/null 2>&1; then
	gcloud iam workload-identity-pools create "${POOL_ID}" \
		--location=global --display-name="GitHub Actions"
fi
if ! gcloud iam workload-identity-pools providers describe "${PROVIDER_ID}" \
	--location=global --workload-identity-pool="${POOL_ID}" >/dev/null 2>&1; then
	gcloud iam workload-identity-pools providers create-oidc "${PROVIDER_ID}" \
		--location=global \
		--workload-identity-pool="${POOL_ID}" \
		--display-name="GitHub OIDC" \
		--issuer-uri="https://token.actions.githubusercontent.com" \
		--attribute-mapping="google.subject=assertion.sub,attribute.repository=assertion.repository" \
		--attribute-condition="assertion.repository == '${GITHUB_REPO}'"
fi
gcloud iam service-accounts add-iam-policy-binding "${SA_EMAIL}" \
	--role="roles/iam.workloadIdentityUser" \
	--member="principalSet://iam.googleapis.com/projects/${PROJECT_NUMBER}/locations/global/workloadIdentityPools/${POOL_ID}/attribute.repository/${GITHUB_REPO}" >/dev/null

WIF_PROVIDER="projects/${PROJECT_NUMBER}/locations/global/workloadIdentityPools/${POOL_ID}/providers/${PROVIDER_ID}"

echo "▶ 5/6 Secret Manager: 시크릿 3종 생성 + 런타임 SA 접근권한"
# 값은 넣지 않는다(버전 등록은 아래 ①에서 실제 값으로). Cloud Run이 env로 마운트한다.
for KEY in database-url jwt-secret enc-key; do
	SID="${SERVICE}-${KEY}"
	if ! gcloud secrets describe "${SID}" >/dev/null 2>&1; then
		gcloud secrets create "${SID}" --replication-policy=automatic
	fi
	gcloud secrets add-iam-policy-binding "${SID}" \
		--member="serviceAccount:${RUNTIME_SA}" \
		--role="roles/secretmanager.secretAccessor" >/dev/null
done

echo "▶ 6/6 남은 수동 단계 안내"
cat <<CMDS

# ── ① 시크릿 실제 값 등록 (필수 — 없으면 서버가 fail-closed로 기동 거부) ──
#   DATABASE_URL 은 setup-cloud-sql.sh 가 자동 등록한다. JWT/ENC 두 개만 등록하면 된다.
printf '%s' '<PSTMACRO_JWT_SECRET 값>' | gcloud secrets versions add ${SERVICE}-jwt-secret --data-file=-
printf '%s' '<PSTMACRO_ENC_KEY 값>'    | gcloud secrets versions add ${SERVICE}-enc-key    --data-file=-

# ── ② GitHub 레포 변수 (워크플로 인증용) ──
gh variable set GCP_WIF_PROVIDER --repo ${GITHUB_REPO} --body "${WIF_PROVIDER}"
gh variable set GCP_DEPLOYER_SA  --repo ${GITHUB_REPO} --body "${SA_EMAIL}"
# ──────────────────────────────────────────────────────────

CMDS
echo "✅ Cloud Run 셋업 완료. Cloud SQL(setup-cloud-sql.sh) → 시크릿 값 등록(①) → GitHub 변수(②) 후 master 푸시 시 첫 배포가 시작됩니다."
