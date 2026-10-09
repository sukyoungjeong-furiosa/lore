---
name: lore-ko
description: Lore가 초기화된 Git checkout 또는 worktree에서 코드에 연결된 외부 지식을 발견하고 읽으며 문서와 코드 사이의 매핑을 유지한다. Notion 문서는 import하지 않고 조회한다.
---

# Lore 사용 예제

이 스킬은 교체 가능한 한국어 예제이며 제품의 필수 워크플로가 아니다. 실제 작업 중인 checkout의 절대 경로를 `--repo`에 전달한다.

## 코드에서 지식 찾기

```bash
lore --repo <checkout> --json status
lore --repo <checkout> --json map ls <task-file-or-directory>
```

결과는 현재 Workspace에 설치된 매핑에 한정된다. 빈 결과가 다른 worktree나 공유 소스에 지식이 없음을 뜻하지 않는다. 초기화되지 않았다면 사용자 요청 범위에서 `init`한다. 다른 worktree의 매핑을 가져올 때는 범위를 지정한다.

```bash
lore --repo <checkout> --json map install --from <other-checkout> --path <task-file-or-directory>
```

각 매핑의 `document.backend`와 `document.id`를 사용해 문서를 읽는다. 본문과 연결 이유를 현재 코드와 비교한다. `map resolve`가 위치를 찾았다고 해서 설명의 정확성이나 사람의 검토가 보증되는 것은 아니다. 원본 Anchor를 현재 위치로 덮어쓰지 않는다.

```bash
lore --repo <checkout> --json docs show <backend> <page-id>
lore --repo <checkout> --json map resolve <mapping-id>
```

## 연결되지 않은 문서 찾기

```bash
lore --repo <checkout> --json backend list
lore --repo <checkout> --json docs browse <backend>
lore --repo <checkout> --json docs browse <backend> <page-id>
```

`docs browse`는 설정한 루트와 하위 문서를 탐색하며 매핑이나 Lore 소유 문서를 생성하지 않는다. 본문은 `docs show`에서 별도로 조회한다. 문서의 Notion 링크를 따라갈 때 같은 backend와 대상 페이지 ID를 사용한다. Notion 원문 도구를 사용해도 된다.

제목 검색에는 미리 만든 인덱스를 사용한다. 인덱스가 없다면 작업에 필요한 범위의 탐색부터 진행한다. 전체 루트의 제목 수집이 필요할 때 `docs fetch <backend>`를 실행한다. 이는 본문 검색이 아니며, 실패한 하위 탐색은 `complete`와 `warnings`에 표시된다.

```bash
lore --repo <checkout> --json docs fetch <backend>
lore --repo <checkout> --json docs search <backend> "scheduler"
```

`docs show/browse/describe`는 캐시를 우선 읽고 없을 때만 조회한다. 최신 내용이 필요하면 `docs fetch <backend> <page-id>`, 트리와 제목 갱신은 `docs fetch <backend>`를 사용한다. `--cached`는 네트워크 없이 읽고 캐시가 없으면 실패한다. 전체 본문이 필요할 때만 `docs fetch <backend> --with-content`를 사용한다. `fetched_at_ms`, `cached`, `partial`, `warnings`를 확인하고 오래된 관측을 현재 사실로 제시하지 않는다. 접근 실패는 삭제나 지식 부재의 증거가 아니다. 토큰을 출력하거나 문서·프롬프트·코드에 기록하지 않는다. VS Code에 저장된 토큰은 CLI에 자동 전달되지 않으므로 CLI 인증은 별도로 준비되어 있어야 한다.

## 매핑 남기기

확인한 코드↔문서 관계를 매핑과 한국어 설명으로 남긴다. 사실과 추론을 구분하고 특정 revision에서만 성립하는 조건을 설명한다. 매핑 생성은 Notion 문서를 수정하지 않으며, 문서를 미리 import할 필요도 없다.

```bash
lore --repo <checkout> --json --author agent map create <backend> <page-id> compiler/scheduler.py:100-150 --context "이 문서는 스케줄러의 자원 제약을 설명한다."
```

편집 전에 revision을 읽고 조건부 저장한다. 충돌하면 현재 상태와 제안한 변경을 비교한 뒤 다시 판단한다. Archive는 매핑만 보관 처리하며 Notion 문서를 archive하지 않는다.

```bash
lore --repo <checkout> --json map show <mapping-id>
lore --repo <checkout> --json --author agent map edit <mapping-id> --if-match <observed-revision> --context "수정한 연결 이유"
lore --repo <checkout> --json map history <mapping-id>
```

현재 서버와 Push/Pull, 모델 호출 기반 Resolver는 구현되어 있지 않다. 모든 매핑 변경은 로컬 Workspace에 기록된다. 기존 Lore 소유 문서 실험 기능은 별도의 `create`, `show`, `ls`, `search` 명령으로 남아 있으며, 이를 외부 문서 조회나 매핑 명령과 혼동하지 않는다.
