"""Correlate receiver pointer events with confirmed native deliveries."""


def pointer_load_proof(report, status, observation_start, operation_baseline):
    operation_baseline = int(operation_baseline)
    moves = [event for event in report.get("observations", [])[observation_start:]
        if event.get("injectedMarker") is True and event["message"] == 512
        and event["foregroundWindow"] == report["focusAfter"] and event["captureWindow"] == "0"]
    receipts = {action: [] for action in ("pointer-a", "pointer-b")}
    outcomes = status["runtime"].get("outcomes", [])
    by_operation = {row["delivery"]["operationId"]: row["acknowledgement"] for row in outcomes}
    for outcome in outcomes:
        delivery, ack = outcome["delivery"], outcome["acknowledgement"]
        operation = delivery["operationId"]
        assert isinstance(operation, str) and operation.isdecimal() and str(int(operation)) == operation, delivery
        if (int(operation) <= operation_baseline or delivery["actionId"] not in receipts
                or delivery["intent"]["kind"] != "pointer"):
            continue
        submission = ack
        if batch := ack.get("pointerBatch"):
            assert operation in batch["operationIds"], outcome
            submission = by_operation.get(batch["operationIds"][0])
            if submission is None:  # The first participant can age out of the outcome buffer.
                continue
            assert submission["sequence"] == ack["sequence"] and submission["pointerBatch"] == batch, outcome
        if submission["attempted"] == 0:
            continue
        for receipt in (ack, submission):
            assert receipt["ok"] and not receipt["uncertain"] and not receipt["skipped"], outcome
            assert receipt["confirmed"] == receipt["attempted"], outcome
        receipts[delivery["actionId"]].append(outcome)
    if not moves or not all(receipts.values()):
        return None
    return dict(receiverMarkedMouseMoves=len(moves), receiverObservationStart=observation_start,
        operationBaseline=str(operation_baseline),
        countBoundary="Recent outcomes only. A combined pointer submission confirms every participant and counts native events only on the first participant.",
        recentNativePointerCounts={action: dict(successfulSubmissions=len(values),
            nativeEventsAttempted=sum(value["acknowledgement"]["attempted"] for value in values),
            nativeEventsConfirmed=sum(value["acknowledgement"]["confirmed"] for value in values)) for action, values in receipts.items()},
        recentNativePointerReceipts=receipts)
