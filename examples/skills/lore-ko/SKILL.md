---
name: lore-ko
description: Lore가 소개한 관련 문서를 읽거나, 코드 경로로 지식을 찾고 문서와 코드의 매핑을 관리할 때 사용한다.
---

# Lore 읽기와 매핑

Lore는 코드에 연결된 문서를 찾고 관계를 기록하는 도구다. `--repo`에는 현재 작업 checkout의 절대 경로를 전달한다.

## 관련 지식 찾기

코드 경로로 관련 문서를 찾는다. 자동 소개에서도 작업에 필요한 문서만 선택해 읽는다.

```bash
lore --repo <checkout> --json context discover <file-or-directory>
```

Discovery는 공유 인덱스와 로컬 변경을 조회하며, 문서 설치·본문 읽기·Projection은 수행하지 않는다. 필요한 인덱스는 원격에서 가져올 수 있다. `--cached`는 네트워크 요청을 금지하며, `warnings`가 있으면 탐색 범위가 불완전할 수 있다.

자동 소개는 세션당 문서별 최대 한 번이다. 명시적 탐색과 읽기는 언제든 가능하다.

## 문서 읽기

`document.source`가 `backend`이면 `reference.backend`와 `reference.id`, `local`이면 `id`로 읽는다.

```bash
lore --repo <checkout> --json docs show <backend> <document-id>
lore --repo <checkout> --json show <local-document-id>
```

- 외부 문서는 캐시를 우선 사용하고, 없으면 가져온다. 최신 본문은 `docs fetch <backend> <document-id>`, 제목 목록 갱신은 `docs fetch <backend>`를 사용한다. `docs search`는 캐시된 제목을 검색한다.
- 캐시는 원본보다 오래될 수 있고, `partial`·`warnings`는 불완전한 결과를 나타낸다. 접근 실패는 문서 삭제를 의미하지 않는다.
- CLI·확장·훅은 같은 Lore home의 인증을 공유한다. 인증 문제는 `backend auth status <backend>`로 확인한다. 토큰은 출력하거나 문서에 기록하지 않는다.

## 코드 연결 유지

파일 범위는 glob, 특정 코드 위치는 Anchor로 연결한다. 연결 이유에는 사실·추론·적용 조건을 구분해 적는다. 매핑 변경은 외부 문서를 수정하지 않는다.

```bash
lore --repo <checkout> --json --author agent map create <backend> <document-id> --glob 'src/scheduler/**' --context "스케줄러 설계 배경"
lore --repo <checkout> --json --author agent map create <backend> <document-id> src/scheduler/plan.rs:10-30 --context "이 함수의 자원 제약"
lore --repo <checkout> --json --author agent map edit <mapping-id> --if-match <observed-revision> --context "수정한 연결 이유"
```

- `matches.scope`는 경로 패턴의 관계이고, `recorded_anchor`의 라인은 기록 당시 위치다. 현재 위치는 `map ls <path>`·`map resolve <mapping-id>`로 조회한다. Projection은 원본 Anchor를 보존하며 설명의 타당성을 보증하지 않는다.
- 편집 기준 revision은 `map show <mapping-id>`로 확인한다. 충돌 시 현재 상태와 변경안을 비교하며, 기존 변경을 덮어쓰지 않는다.
- 작성·수정은 해당 workspace에 남는다. 공유 게시는 별도 작업이며 revision 조건은 `map publish --help`로 확인한다. 다른 workspace의 매핑을 설치하는 방법과 추가 옵션은 `map --help`를 참고한다.
