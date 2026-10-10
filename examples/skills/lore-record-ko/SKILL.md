---
name: lore-record-ko
description: Stop 훅이 기록 점검을 요청하거나 사용자가 Lore에 지식을 남기라고 할 때 사용한다. 새 결론을 기록하고 작업 브랜치에서 로컬 중앙에 제출한다.
---

# Lore 지식 기록

완료한 작업에서 재사용할 새 지식이 있으면 기록하고 **커밋 → 로컬 중앙 제출**까지 마친다. 원격 제출은 별도 지시가 있어야 한다. `--repo`는 실제 작업 checkout의 절대 경로, 작성자는 `--author agent`를 사용한다.

## 기록 판단

코드만 읽어서는 알기 어려운 설계 이유, 적용 조건, 제약, 검증된 조사·실험 결론을 남긴다. 일반적인 코드 설명, 진행 상황, 단순 테스트 통과만으로는 기록하지 않는다. 후보가 없으면 도구 호출 없이 종료한다.

1. `context discover`와 `search`로 같은 주제의 문서를 찾아 읽는다. 중앙 문서는 `--central show <id>`, 외부 문서는 `docs show <backend> <id>`로 읽는다.
2. `status`와 `worktree`로 기존 작업을 확인한다. 다른 작업의 미커밋 변경이나 미제출 커밋을 함께 게시하지 않는다. 기존 문서를 수정할 때는 `pull`로 중앙 변경을 반영하고 자기 브랜치의 문서를 다시 읽는다. 충돌을 자동 해결하지 않는다.
3. 사용자의 언어로 결론과 적용 범위를 먼저 적고 이유와 근거를 짧게 붙인다. 확인한 사실과 가설을 구분한다. 코드 연결은 핵심 위치만 남기고, 현재 코드가 실험 당시 코드인 것처럼 연결하지 않는다.
4. 기록과 연결을 마치면 선택한 문서를 커밋하고 로컬 중앙에 제출한다. 편집·연결·커밋에는 직전에 반환된 `revision`을 사용한다.

```bash
lore --repo <checkout> --json --author agent create "스케줄러의 자원 제약" --stdin
lore --repo <checkout> --json --author agent edit <document-id> --if-match <revision> --stdin
lore --repo <checkout> --json --author agent attach <document-id> src/scheduler/plan.rs:10-30 --if-match <revision>
lore --repo <checkout> --json --author agent commit <document-id> --if-match <final-revision>
lore --repo <checkout> --json push
```

`--stdin`은 전체 본문이다. 새 문서의 ID는 `document.id`에서 얻는다. 다른 Local Document는 `[[id:<document-id>|표시 이름]]`으로 참조한다.

커밋은 자기 작업 브랜치에만 남는다. Push가 로컬 중앙에 통합되어야 중앙 화면과 다른 작업자가 발견할 수 있다. 다른 작업 브랜치는 Pull로 반영한다. `push --proposal`은 통합 없이 검토할 제안을 남긴다.

충돌하면 문서와 제안을 보존하고 점검을 마친다. `proposal show <id>`로 baseline·제안·중앙을 비교할 수 있다. baseline을 임의로 바꾸거나 `resolve-conflict --take local`로 자동 덮어쓰지 않는다. 명시적으로 해결을 요청받은 경우에만 관찰한 revision 조건으로 적용한다.

명시적 기록 요청에서 workspace가 없으면 `lore init`으로 준비한다. CLI가 없거나 실패하면 자동 설치나 반복 재시도하지 않는다. 문서 제목·ID와 **중앙 통합 완료 / 제출 대기 / 커밋만 완료 / 작성만 완료**를 구분해 짧게 알린다. 새 지식이 없으면 조용히 종료하고 원래 작업을 다시 시작하지 않는다.
