"""Pure, file-only proof-summary regressions; never use serial ports or devices."""
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

path = Path(__file__).resolve().parents[1] / "tools/render-proof.py"
spec = importlib.util.spec_from_file_location("render_proof", path)
proof = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proof)


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def catalog():
    return {sha(board + revision): dict(board=board, revision=revision, boardProfile=profiles[0],
        targetProfileId=profiles[1], profileHash=sha(board + "profile"), packageHash=sha(board + revision),
        sourcePublicationHash=sha(board + revision + "source"))
        for board, profiles in proof.BOARD_PROFILES.items() for revision in ("A", "B")}


def identity(board="sunton", revision="A", identities=None):
    return next(v for v in (identities or catalog()).values() if v["board"] == board and v["revision"] == revision)


def published(board="sunton", revision="A", identities=None):
    item = identity(board, revision, identities)
    return dict(event="published", board=board, revision=revision,
                packageHash=item["packageHash"], publicationHash=item["sourcePublicationHash"])


def live(time=0, revision="A", session="session-a", content=1000, reads=1, identities=None):
    item = identity(revision=revision, identities=identities)
    return dict(event="device", elapsedMs=time, revision=revision, port="COM6",
        contentBytes=content, contentRequests=reads, deviceStatus="live-armed", lastHostError=None,
        device=dict(boardProfile=item["boardProfile"], status="live-armed",
            renderPackageHash=item["packageHash"], publicationHash=item["sourcePublicationHash"],
            sessionId=session, generation="1", token="secret-never-output", ssid="private-network"))


def finish():
    return dict(event="complete", revision="B", counts=dict(baselines=4, sourceLosses=2))


def installer(digest="a"*64, size=1000, readonly=True, port="COM6"):
    return dict(profile="sunton-3248s035r", applicationSha256=digest,
        expectedApplicationSha256=digest, applicationBytes=size, readOnly=readonly, port=port,
        writtenOffsets=[] if readonly else [0,32768,65536],
        segments=[dict(name="application", offset=65536, bytes=size, sha256=digest, expectedSha256=digest)],
        password="secret-never-output", nvsMd5="private-ignored")


class RenderProofTests(unittest.TestCase):
    def test_actual_revisions_sessions_interruptions_and_sampling_gaps(self):
        delayed = live(2100)
        delayed["revision"] = "B"  # requested B while A is still the actual panel
        disconnected = live(3100)
        disconnected["deviceStatus"] = "disconnected"
        rows = [published(), live(100), live(1100), published(revision="B"), delayed, disconnected,
                live(4000,"B","session-b",2000,2), live(5000,"B","session-b",2000,2),
                live(9000,"B","session-b",2000,2), live(10000,"B","session-c",2000,2), finish()]
        result = proof.summarize(rows, catalog())
        board = result["boards"][0]
        self.assertTrue(result["runComplete"])
        self.assertEqual(board["sessionCount"],3)
        self.assertEqual(board["longestContinuousLiveMs"],2000)
        self.assertEqual([v["revision"] for v in board["liveEvents"]],["A","B","B","B"])
        self.assertEqual(board["consecutiveLiveContent"]["byteIncrement"],0)
        self.assertTrue(board["consecutiveLiveContent"]["allZero"])
        self.assertEqual((board["lastContentBytes"],board["lastContentRequests"]),(2000,2))
        self.assertEqual(result["inputEvidenceCounts"],dict(baselines=4,sourceLosses=2))
        self.assertNotIn("secret-never-output",json.dumps(result))
        self.assertNotIn("private-network",json.dumps(result))
        self.assertEqual(board["application"]["status"],"not-provided")
        self.assertIn("soak qualification",result["notEstablished"])

    def test_nonzero_content_reset_and_missing_counters_never_cancel_or_verify(self):
        rows = [published(),live(0,content=100,reads=1),live(1000,content=120,reads=2),
                live(2000,content=110,reads=2),live(3000,content=None,reads=None),finish()]
        result = proof.summarize(rows,catalog())["boards"][0]["consecutiveLiveContent"]
        self.assertEqual(result,dict(pairs=3,measuredPairs=2,byteIncrement=20,requestIncrement=1,
                                    counterRegressions=1,missingCounterPairs=1,allZero=False))
        single = proof.summarize([published(),live(),finish()],catalog())["boards"][0]
        self.assertIsNone(single["consecutiveLiveContent"]["allZero"])
        self.assertEqual(single["longestContinuousLiveMs"],0)

    def test_null_or_host_failure_records_break_live_continuity(self):
        empty = dict(event="device",port="COM6",elapsedMs=1500,device=None,contentBytes=None,contentRequests=None)
        failed = live(3500)
        failed["lastHostError"] = "secret-never-output"
        rows = [published(),live(),live(1000),empty,live(2000),live(3000),failed,live(4000),finish()]
        result = proof.summarize(rows,catalog())["boards"][0]
        self.assertEqual(len(result["liveEvents"]),3)
        self.assertEqual(result["longestContinuousLiveMs"],1000)
        self.assertNotIn("secret-never-output",json.dumps(result))

    def test_invalid_identity_time_counter_and_unknown_events_fail_closed(self):
        mutations = [
            lambda v:v["device"].update(boardProfile="unknown"),
            lambda v:v["device"].update(publicationHash=sha("other")),
            lambda v:v["device"].update(renderPackageHash="sha256:bad"),
            lambda v:v["device"].update(sessionId="unsafe id"),
            lambda v:v["device"].update(generation="01"),
            lambda v:v["device"].update(generation="18446744073709551616"),
            lambda v:v.update(contentBytes=True),
            lambda v:v.update(contentRequests=-1),
            lambda v:v.update(elapsedMs=1.5),
            lambda v:v.update(port="COM\n6"),
        ]
        for mutate in mutations:
            value=live()
            mutate(value)
            with self.subTest(value=value),self.assertRaises(ValueError):
                proof.summarize([published(),value,finish()],catalog())
        with self.assertRaises(ValueError):
            proof.summarize([published(),live(100),live(99),finish()],catalog())
        with self.assertRaises(ValueError):
            proof.summarize([published(),dict(event="unknown"),finish()],catalog())
        with self.assertRaises(ValueError):
            proof.summarize([published(),finish(),live()],catalog())
        bad=published();bad["packageHash"]=sha("other")
        with self.assertRaises(ValueError):
            proof.summarize([bad,finish()],catalog())
        with self.assertRaises(ValueError):
            proof.summarize([published(),live()],catalog())
        result=proof.summarize([published(),live()],catalog(),allow_running=True)
        self.assertFalse(result["runComplete"])

    def test_file_limits_duplicate_keys_nonfinite_and_incomplete_final_write(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/"capture.jsonl"
            valid=json.dumps(published()).encode()+b"\n"
            for content in [b'{"event":"complete","event":"published"}\n',b'{"event":NaN}\n',
                            b'{"event":1e999}\n',b'[]\n',b'\xff\n',b'{"event":\n',
                            b'x'*(proof.MAX_LINE+1)+b'\n',valid+b'{"event":"complete"']:
                path.write_bytes(content)
                with self.subTest(content=content[:40]),self.assertRaises(ValueError):
                    list(proof.read_records(path))
            path.write_bytes(valid+b'{"event":"complete"')
            self.assertEqual(list(proof.read_records(path,True)),[published()])
            path.write_bytes(b' '* (proof.MAX_FILE+1))
            with self.assertRaises(ValueError):
                list(proof.read_records(path))
            path.write_bytes(valid)
            self.assertEqual(list(proof.read_records(path)),[published()])

    def test_installer_whole_image_readback_missing_changed_and_connection_cases(self):
        board,before=proof.installer_record(installer(readonly=False))
        _,after=proof.installer_record(installer())
        result=proof.application_proof(board,"COM6",{board:before},{board:after})
        self.assertTrue(result["unchangedVerified"])
        self.assertEqual(result["status"],"unchanged")
        self.assertNotIn("secret-never-output",json.dumps(result))
        self.assertEqual(proof.application_proof(board,"COM6",{board:before},{})["status"],"missing-after")
        self.assertFalse(proof.application_proof(board,"COM6",{board:before},{})["unchangedVerified"])
        self.assertEqual(proof.application_proof(board,"COM6",{},{board:after})["status"],"missing-before")
        _,different=proof.installer_record(installer("b"*64))
        self.assertEqual(proof.application_proof(board,"COM6",{board:before},{board:different})["status"],"changed")
        self.assertEqual(proof.application_proof(board,"COM5",{board:before},{board:after})["status"],"connection-mismatch")
        with self.assertRaises(ValueError):
            proof.application_proof(board,"COM6",{board:before},{board:before})
        for mutate in [
            lambda v:v.update(profile="unknown"),lambda v:v.update(applicationSha256="invalid"),
            lambda v:v.update(applicationBytes=0),lambda v:v.update(expectedApplicationSha256="b"*64),
            lambda v:v["segments"][0].update(offset=65537),
            lambda v:v["segments"][0].update(bytes=999),
            lambda v:v.update(writtenOffsets=[65536]),
            lambda v:v["segments"].append(copy.deepcopy(v["segments"][0])),
        ]:
            value=installer();mutate(value)
            with self.assertRaises(ValueError):
                proof.installer_record(value)

    def write_fixtures(self,root):
        for item in catalog().values():
            directory=root/item["board"]/item["revision"]
            directory.mkdir(parents=True)
            manifest=dict(targetProfileId=item["targetProfileId"],targetProfileHash=item["profileHash"],
                          sourcePublicationHash=item["sourcePublicationHash"])
            raw=json.dumps(manifest,sort_keys=True,separators=(",",":")).encode()
            identity=copy.deepcopy(item)
            identity["packageHash"]="sha256:"+hashlib.sha256(raw).hexdigest()
            identity["password"]="secret-never-output"
            (directory/"identity.json").write_text(json.dumps(identity),encoding="utf-8")
            (directory/"manifest.json").write_bytes(raw)
        return proof.load_identities(root)

    def test_fixture_manifest_hash_binding_and_cli_output_are_reproducible_and_redacted(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)/"fixtures"
            identities=self.write_fixtures(root)
            path=Path(directory)/"capture.jsonl"
            rows=[published(identities=identities),live(identities=identities),live(1000,identities=identities),finish()]
            path.write_text("".join(json.dumps(v)+"\n" for v in rows),encoding="utf-8")
            output=Path(directory)/"proof.json"
            self.assertEqual(proof.main([str(path),"--fixtures",str(root),"--output",str(output)]),0)
            first=output.read_bytes()
            self.assertEqual(proof.main([str(path),"--fixtures",str(root),"--output",str(output)]),0)
            self.assertEqual(first,output.read_bytes())
            self.assertNotIn(b"secret-never-output",first)
            path.write_text('{"secret":"secret-never-output"}\n',encoding="utf-8")
            error=io.StringIO()
            with contextlib.redirect_stderr(error):
                self.assertEqual(proof.main([str(path),"--fixtures",str(root),"--output",str(output)]),2)
            self.assertNotIn("secret-never-output",error.getvalue())
            (root/"sunton"/"A"/"manifest.json").write_text("{}",encoding="utf-8")
            with self.assertRaises(ValueError):
                proof.load_identities(root)

    def test_boards_and_connection_failures_are_independent(self):
        wave0=live()
        wave1=live(1000)
        wave_identity=identity("waveshare","A")
        for value in (wave0,wave1):
            value["port"]="COM5"
            value["device"].update(boardProfile=wave_identity["boardProfile"],
                renderPackageHash=wave_identity["packageHash"],
                publicationHash=wave_identity["sourcePublicationHash"],sessionId="wave-session")
        rows=[dict(event="device",port="COM5",elapsedMs=0,device=None),
              published(),published("waveshare"),live(),wave0,live(1000),wave1,finish()]
        result=proof.summarize(rows,catalog())
        self.assertEqual([r["board"] for r in result["boards"]],["sunton","waveshare"])
        self.assertEqual([r["longestContinuousLiveMs"] for r in result["boards"]],[1000,1000])
        self.assertTrue(all(r["consecutiveLiveContent"]["allZero"] for r in result["boards"]))
        repeated=copy.deepcopy(wave1)
        repeated["port"]="COM7"
        with self.assertRaises(ValueError):
            proof.summarize(rows[:-1]+[repeated,finish()],catalog())
        swapped=copy.deepcopy(wave1)
        swapped["port"]="COM6"
        with self.assertRaises(ValueError):
            proof.summarize(rows[:-1]+[swapped,finish()],catalog())
    def test_fixed_device_events_break_continuity_and_expose_sequence_gaps(self):
        def event(sequence,time,status,port="COM6"):
            return dict(event="device-event",port=port,sequence=sequence,
                        elapsedMs=time,transportElapsedMs=time,status=status,token="secret-never-output")
        rows=[published(),event(1,0,"render-package-verified"),live(100),live(1100),
              event(3,1200,"source-host-rejected-reconnect-required"),
              event(4,1300,"arming"),event(5,1400,"live-armed"),live(2000),live(3000),finish()]
        result=proof.summarize(rows,catalog())["boards"][0]
        self.assertEqual(result["longestContinuousLiveMs"],1000)
        self.assertEqual(len(result["liveEvents"]),2)
        self.assertEqual(result["deviceEventSequenceGaps"],1)
        self.assertEqual(result["deviceEventCounts"]["source-host-rejected-reconnect-required"],1)
        self.assertEqual(len(result["deviceEvents"]),4)
        self.assertNotIn("secret-never-output",json.dumps(result))
        for bad in [event(1,0,"secret-never-output"),event(0,0,"live-armed"),
                    event(True,0,"live-armed")]:
            with self.assertRaises(ValueError):
                proof.summarize([published(),bad,finish()],catalog())
        with self.assertRaises(ValueError):
            proof.summarize([published(),live(100),event(1,0,"live-armed"),finish()],catalog())
        with self.assertRaises(ValueError):
            proof.summarize([published(),event(1,0,"live-armed"),event(1,1,"live-armed"),finish()],catalog())
        result=proof.summarize([published(),event(1,0,"live-armed","COM7"),finish()],catalog())
        self.assertEqual(result["unattributedDeviceEventCount"],1)
        with self.assertRaises(ValueError):
            proof.summarize([published()]+[event(i+1,i,"live-armed","COM7") for i in range(257)]+[finish()],catalog())

    def test_optional_ack_and_exact_serial_health_maxima_are_measured_and_bounded(self):
        zero={key:0 for key in proof.SERIAL_HEALTH}
        first=live()
        first.update(startupAckMaxBytes=400,steadyAckMaxBytes=390)
        first["device"]["serialHealth"]=zero.copy()
        second=live(1000)
        second.update(startupAckMaxBytes=399,steadyAckMaxBytes=398)
        second["device"]["serialHealth"]={**zero,"fifoOverflows":2,"requestTimeouts":1}
        third=live(2000)
        third["device"]["serialHealth"]=zero.copy()
        result=proof.summarize([published(),first,second,third,finish()],catalog())["boards"][0]
        self.assertEqual(result["startupAckMaxBytes"],400)
        self.assertEqual(result["steadyAckMaxBytes"],398)
        self.assertEqual(result["serialHealthMax"]["fifoOverflows"],2)
        self.assertEqual(result["serialHealthMax"]["requestTimeouts"],1)
        self.assertEqual(result["serialHealthSamples"],3)
        self.assertEqual(result["serialHealthCounterRegressions"],2)
        old=proof.summarize([published(),live(content=None,reads=None),
             live(1000,content=None,reads=None),finish()],catalog())["boards"][0]
        self.assertIsNone(old["serialHealthMax"])
        self.assertIsNone(old["startupAckMaxBytes"])
        self.assertIsNone(old["steadyAckMaxBytes"])
        self.assertIsNone(old["consecutiveLiveContent"]["allZero"])
        self.assertIsNone(old["consecutiveLiveContent"]["byteIncrement"])
        self.assertIsNone(old["consecutiveLiveContent"]["requestIncrement"])
        for bad in [True,-1,1.5,4294967296]:
            value=live()
            value["device"]["serialHealth"]={**zero,"fifoOverflows":bad}
            with self.assertRaises(ValueError):
                proof.summarize([published(),value,finish()],catalog())
            value=live()
            value["startupAckMaxBytes"]=bad
            with self.assertRaises(ValueError):
                proof.summarize([published(),value,finish()],catalog())
        for bad in [{**zero,"secret":"secret-never-output"},dict(list(zero.items())[:6]),[]]:
            value=live();value["device"]["serialHealth"]=bad
            with self.assertRaises(ValueError):
                proof.summarize([published(),value,finish()],catalog())

    def test_per_instance_input_counts_whitelist_and_no_raw_input_values(self):
        final=finish()
        final["counts"].update(inputBatches=2,lastChanges=[dict(controlId="secret-never-output",value=True)],
              instances={"render-poc-sunton":dict(baselines=1,inputBatches=2,password="secret-never-output"),
                         "render-poc-waveshare":dict(baselines=3,sourceLosses=2),
                         "secret-never-output":dict(inputBatches=999)})
        result=proof.summarize([published(),live(),final],catalog())
        self.assertEqual(result["inputEvidenceByInstance"],{
             "render-poc-sunton":dict(baselines=1,inputBatches=2),
             "render-poc-waveshare":dict(baselines=3,sourceLosses=2)})
        self.assertEqual(result["boards"][0]["inputEvidenceCounts"],dict(baselines=1,inputBatches=2))
        self.assertNotIn("secret-never-output",json.dumps(result))
        for bad in [True,-1,0.5]:
            value=copy.deepcopy(final)
            value["counts"]["instances"]["render-poc-sunton"]["inputBatches"]=bad
            with self.assertRaises(ValueError):
                proof.summarize([published(),live(),value],catalog())

    def test_first_live_health_and_deltas_exclude_startup_and_new_package_download(self):
        base={key:0 for key in proof.SERIAL_HEALTH}
        startup=live()
        startup["device"]["status"]="opening-panel"
        startup["device"]["serialHealth"]={**base,"breaks":100,"queueFullObservations":6,"requestTimeouts":1}
        first=live(1000)
        first["device"]["serialHealth"]={**base,"breaks":101,"queueFullObservations":6,"requestTimeouts":1}
        second=copy.deepcopy(first);second["elapsedMs"]=2000
        download=live(3000)
        download["device"]["status"]="opening-panel"
        download["device"]["serialHealth"]={**base,"breaks":111,"queueFullObservations":8,"requestTimeouts":2}
        bfirst=live(4000,"B","session-b")
        bfirst["device"]["serialHealth"]=copy.deepcopy(download["device"]["serialHealth"])
        bsecond=copy.deepcopy(bfirst);bsecond["elapsedMs"]=5000
        for row in (startup,first,second,download,bfirst,bsecond):
            row["device"]["firmwareVersion"]="2026.10.17.1"
        result=proof.summarize([published(),startup,first,second,published(revision="B"),
                                download,bfirst,bsecond,finish()],catalog())["boards"][0]
        self.assertEqual(result["serialHealthMax"]["breaks"],111)
        self.assertEqual(result["firstLiveSerialHealth"]["elapsedMs"],1000)
        self.assertEqual(result["firstLiveSerialHealth"]["counters"]["breaks"],101)
        self.assertEqual([v["firstSerialHealth"]["breaks"] for v in result["liveEvents"]],[101,111])
        self.assertEqual(result["consecutiveLiveSerialHealth"]["pairs"],2)
        self.assertEqual(result["consecutiveLiveSerialHealth"]["measuredPairs"],2)
        self.assertEqual(result["consecutiveLiveSerialHealth"]["increments"],base)
        self.assertTrue(result["consecutiveLiveSerialHealth"]["allZero"])
        self.assertEqual(result["firmwareVersions"],["2026.10.17.1"])
        self.assertTrue(all(v["firmwareVersion"]=="2026.10.17.1" for v in result["liveEvents"]))

    def test_live_health_resets_exclude_the_entire_pair_without_canceling_increments(self):
        base={key:0 for key in proof.SERIAL_HEALTH}
        first=live()
        first["device"]["serialHealth"]={**base,"breaks":100,"requestTimeouts":10}
        reset=live(1000)
        reset["device"]["serialHealth"]={**base,"breaks":90,"requestTimeouts":12}
        resumed=live(2000)
        resumed["device"]["serialHealth"]={**base,"breaks":91,"requestTimeouts":13}
        result=proof.summarize([published(),first,reset,resumed,finish()],catalog())["boards"][0]["consecutiveLiveSerialHealth"]
        self.assertEqual(result["measuredPairs"],1)
        self.assertEqual(result["counterRegressionPairs"],1)
        self.assertEqual(result["counterRegressionsByField"]["breaks"],1)
        self.assertEqual(result["increments"]["breaks"],1)
        self.assertEqual(result["increments"]["requestTimeouts"],1)
        self.assertFalse(result["allZero"])
        unknown=proof.summarize([published(),first,reset,finish()],catalog())["boards"][0]["consecutiveLiveSerialHealth"]
        self.assertEqual(unknown["counterRegressionPairs"],1)
        self.assertTrue(all(value is None for value in unknown["increments"].values()))
        self.assertIsNone(unknown["allZero"])

    def test_missing_live_health_remains_unknown_and_first_anchor_is_not_backfilled(self):
        base={key:0 for key in proof.SERIAL_HEALTH}
        first=live()
        second=live(1000);second["device"]["serialHealth"]={**base,"breaks":3}
        third=copy.deepcopy(second);third["elapsedMs"]=2000
        result=proof.summarize([published(),first,second,third,finish()],catalog())["boards"][0]
        self.assertIsNone(result["firstLiveSerialHealth"]["counters"])
        self.assertIsNone(result["liveEvents"][0]["firstSerialHealth"])
        counters=result["consecutiveLiveSerialHealth"]
        self.assertEqual((counters["pairs"],counters["measuredPairs"],counters["missingCounterPairs"]),(2,1,1))
        self.assertEqual(counters["increments"],base)
        self.assertIsNone(counters["allZero"])
        self.assertIsNone(result["firmwareVersions"])
        changed=copy.deepcopy(second);changed["device"]["sessionId"]="another-session"
        boundary=proof.summarize([published(),first,changed,finish()],catalog())["boards"][0]["consecutiveLiveSerialHealth"]
        self.assertEqual(boundary["pairs"],0)
        self.assertTrue(all(value is None for value in boundary["increments"].values()))
        for bad in ["secret-never-output","2026.10.17.1-token",True,0,{}]:
            row=live();row["device"]["firmwareVersion"]=bad
            with self.assertRaises(ValueError):
                proof.summarize([published(),row,finish()],catalog())
        old=live();old["device"]["firmwareVersion"]="0.1.0"
        self.assertEqual(proof.summarize([published(),old,finish()],catalog())["boards"][0]["firmwareVersions"],["0.1.0"])
    def test_installer_file_loading_rejects_duplicate_profiles(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/"installer.json"
            path.write_text(json.dumps(installer()),encoding="utf-8")
            self.assertEqual(set(proof.load_installers([path])),{"sunton"})
            with self.assertRaises(ValueError):
                proof.load_installers([path,path])


if __name__=="__main__":
    unittest.main()