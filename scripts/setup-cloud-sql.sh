#!/usr/bin/env bash
# 원타임 Cloud SQL 셋업: PostgreSQL 인스턴스 1개(pstmacro-db) 생성, pstmacro DB·유저 생성,
# DATABASE_URL(유닉스 소켓 형식)을 Secret Manager(pstmacro-server-database-url)에 자동 등록.
# (bambi-app/scripts/setup-cloud-sql.sh 를 pstmacro 용으로 적응 — 단일 인스턴스.)
# Cloud Run과는 deploy-server.yml 의 --add-cloudsql-instances(유닉스 소켓)로 연결된다.
#
# 사전: setup-gcp-cloud-run.sh 를 먼저 실행(시크릿 껍데기 생성). 실행: bash scripts/setup-cloud-sql.sh
#       (인스턴스 생성에 10~20분 소요)
set -euo pipefail

PROJECT_ID="${PROJECT_ID:-pstmacro-gcp}"
REGION="${REGION:-asia-northeast3}"
SERVICE="${SERVICE:-pstmacro-server}"
INSTANCE="${INSTANCE:-pstmacro-db}"
DB_VERSION="POSTGRES_16"
DB_NAME="pstmacro"
DB_USER="pstmacro"
SECRET_ID="${SERVICE}-database-url"

gcloud config set project "${PROJECT_ID}" >/dev/null

echo "▶ 1/4 Cloud SQL Admin API 활성화 + 런타임 SA에 cloudsql.client"
gcloud services enable sqladmin.googleapis.com
PROJECT_NUMBER="$(gcloud projects describe "${PROJECT_ID}" --format='value(projectNumber)')"
RUNTIME_SA="${PROJECT_NUMBER}-compute@developer.gserviceaccount.com"
gcloud projects add-iam-policy-binding "${PROJECT_ID}" \
	--member="serviceAccount:${RUNTIME_SA}" --role="roles/cloudsql.client" --condition=None >/dev/null

echo "▶ 2/4 인스턴스 생성 (${DB_VERSION}, ${REGION}) — 수 분 소요"
if gcloud sql instances describe "${INSTANCE}" >/dev/null 2>&1; then
	echo "  - ${INSTANCE}: 이미 존재, 건너뜀"
else
	# 최소 사양(공유코어). 운영 규모 커지면 --tier 상향 + 백업/PITR 추가.
	gcloud sql instances create "${INSTANCE}" \
		--database-version="${DB_VERSION}" \
		--region="${REGION}" \
		--edition=enterprise \
		--tier=db-g1-small \
		--storage-size=10GB \
		--storage-auto-increase
fi

echo "▶ 3/4 DB·유저 생성 + DATABASE_URL 시크릿 등록"
if ! gcloud sql databases describe "${DB_NAME}" --instance="${INSTANCE}" >/dev/null 2>&1; then
	gcloud sql databases create "${DB_NAME}" --instance="${INSTANCE}"
fi
EXISTING_USERS="$(gcloud sql users list --instance="${INSTANCE}" --format='value(name)')"
if printf '%s\n' "${EXISTING_USERS}" | grep -qx "${DB_USER}"; then
	echo "  - 유저 ${DB_USER} 이미 존재 — 비밀번호·시크릿 갱신 생략"
else
	# 파이프라인 없이 생성(pipefail + head 조합의 SIGPIPE 회피).
	PASS="$(openssl rand -hex 16)"
	gcloud sql users create "${DB_USER}" --instance="${INSTANCE}" --password="${PASS}"
	# 시크릿 껍데기(setup-gcp-cloud-run.sh 가 생성)가 없으면 만든다.
	if ! gcloud secrets describe "${SECRET_ID}" >/dev/null 2>&1; then
		gcloud secrets create "${SECRET_ID}" --replication-policy=automatic
		gcloud secrets add-iam-policy-binding "${SECRET_ID}" \
			--member="serviceAccount:${RUNTIME_SA}" \
			--role="roles/secretmanager.secretAccessor" >/dev/null
	fi
	# Cloud Run 유닉스 소켓 경유 URL(pg가 host 쿼리 파라미터를 소켓 경로로 해석).
	printf '%s' "postgresql://${DB_USER}:${PASS}@localhost/${DB_NAME}?host=/cloudsql/${PROJECT_ID}:${REGION}:${INSTANCE}" \
		| gcloud secrets versions add "${SECRET_ID}" --data-file=-
	echo "  - DB·유저 생성 + ${SECRET_ID} 시크릿 등록 완료"
fi

echo "▶ 4/4 연결 이름"
gcloud sql instances list --format="table(name,connectionName,databaseVersion,state)"
echo "✅ Cloud SQL 셋업 완료. (스키마는 서버가 첫 연결 시 자동 생성 — 별도 마이그레이션 불필요.)"
