---
name: lore-ko
description: Lore의 관련 문서 목록을 받았거나 코드에 연결된 지식을 조회·읽고 매핑을 유지할 때 사용하는 한국어 예제. 외부 문서는 소유 백엔드에서 읽으며 필수 작업 순서를 정하지 않는다.
---

# Lore 읽기와 매핑

교체 가능한 예제 스킬이다. 실제 작업 checkout의 절대 경로를 `--repo`에 전달한다. 자동 소개 목록에서는 작업에 관련된 문서를 선택해 읽는다. 모든 문서를 읽을 필요는 없다.

## 소개된 문서 읽기

목록의 `document.source`가 `backend`이면 `reference.backend`와 `reference.id`, `local`이면 `id`를 사용한다.

```bash
lore --repo <checkout> --json docs show <backend> <document-id>
lore --repo <checkout> --json show <local-document-id>
```

`matches`의 `scope`는 glob으로 발견한 관계다. `recorded_anchor`의 라인은 기록 당시 위치이며 현재 위치나 내용의 타당성을 보증하지 않는다. 본문·연결 이유를 현재 코드와 비교한다. 현재 위치가 필요하면 매핑을 찾고 Projection을 조회한다. 원본 Anchor를 추정 위치로 덮어쓰지 않는다.

```bash
lore --repo <checkout> --json map ls <file-or-directory>
lore --repo <checkout> --json map resolve <mapping-id>
```

## 명시적 발견과 캐시

자동 소개의 중복 방지는 명시적 조회를 제한하지 않는다. 자동 소개는 캐시와 로컬 변경을 조회하며 본문을 읽지 않는다. 명시적 discovery는 필요한 공유 인덱스를 원격에서 가져올 수 있다.

```bash
lore --repo <checkout> --json context discover <file-or-directory>
lore --repo <checkout> --json backend list
lore --repo <checkout> --json docs browse <backend> [page-id]
lore --repo <checkout> --json docs search <backend> "scheduler"
```

`context discover`는 공유 Mapping 인덱스와 로컬 변경의 glob·원본 Anchor를 조회한다. 공유 인덱스가 없으면 원격에서 가져오며, `--cached`는 네트워크 없이 조회한다. `shared_available`, `shared_fetched_at_ms`, `warnings`를 확인한다. 인덱스 갱신은 `map fetch`다. 발견한 관계가 현재 workspace에 설치되었다는 뜻은 아니다. 현재 활성 Mapping의 Projection은 `map ls`·`map resolve`, 다른 worktree에서 수정·Projection할 Mapping을 설치할 때는 `map install --from <other-checkout> --path <scope>`를 사용한다.

`docs browse`는 문서를 import하지 않고 탐색한다. `docs search`는 캐시된 제목 검색이다. `docs show/browse/describe`는 캐시를 우선 사용하며 없을 때 조회한다. `--cached`는 네트워크 요청을 금지한다. 최신 본문은 `docs fetch <backend> <page-id>`, 트리·제목 갱신은 `docs fetch <backend>`로 요청한다. `fetched_at_ms`, `partial`, `warnings`를 확인한다. 접근 실패나 오래된 캐시를 문서 삭제·현재 사실의 증거로 삼지 않는다.

CLI·확장·훅은 같은 Lore home의 전역 인증을 사용한다. `lore backend auth status <backend>`는 토큰 값 없이 인증 유무를 확인한다. 인증 설정은 확장의 **Lore: Set Notion Connection Token** 또는 `lore backend auth set <backend> --stdin`을 사용한다. 환경변수는 해당 프로세스에서 저장된 인증을 덮어쓴다. 토큰을 출력하거나 지식에 기록하지 않는다.

## 매핑 유지

확인한 관계를 코드 범위 또는 Anchor에 연결한다. 사실과 추론, 특정 코드 상태에만 성립하는 조건을 연결 이유에 설명한다. 매핑을 만들어도 외부 문서는 수정되지 않는다.

```bash
lore --repo <checkout> --json --author agent map create <backend> <document-id> --glob 'src/scheduler/**' --context "스케줄러 설계 배경"
lore --repo <checkout> --json --author agent map create <backend> <document-id> src/scheduler/plan.rs:10-30 --context "이 함수의 자원 제약"
lore --repo <checkout> --json map show <mapping-id>
lore --repo <checkout> --json --author agent map edit <mapping-id> --if-match <observed-revision> --context "수정한 연결 이유"
```

편집은 관측한 revision을 조건으로 저장한다. 충돌하면 현재 상태와 제안한 변경을 비교한 뒤 다시 판단한다. `map history`로 변경 이력을 확인한다. 매핑 Archive는 외부 문서를 archive하지 않는다. 작성·수정은 로컬 workspace에 기록된다. 로컬 공유 인덱스로 게시할 때는 `map publish <id> --if-match <local-revision>`을 명시적으로 사용한다. 이미 게시한 Mapping 갱신에는 `--if-shared <observed-shared-revision>`도 필요하다. 원격 인덱스는 현재 읽기 전용이며 원격 Push/Pull은 아직 없다.
