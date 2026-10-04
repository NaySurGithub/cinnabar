#!/usr/bin/env python3
"""Run an existing test APK on a bounded KVM emulator and save runtime evidence."""

import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shlex
import signal
import subprocess
import time
import xml.etree.ElementTree as ET

RUNTIME = Path(__file__).with_name('runtime.json')
PORT = 5554
SERVER_NAME = 'Smoke target'
SERVER_CONFIG = 'files/config/servers.json'


def observed_home_play(value):
    if not value:
        return None
    match = re.fullmatch(r'(\d+),(\d+)@(\d+)x(\d+)', value)
    if match:
        x, y, width, height = map(int, match.groups())
        if 0 <= x < width and 0 <= y < height:
            return x, y, width, height
    raise argparse.ArgumentTypeError('observed home Play must be x,y@WIDTHxHEIGHT within that frame')


def text_target(tsv, label, placement='unique', width=None):
    """Locate observed text; OreUI's saved row is left and its hero Play is right."""
    lines = {}
    for word in csv.DictReader(io.StringIO(tsv), delimiter='\t'):
        if not word.get('text', '').strip() or float(word['conf']) < 50:
            continue
        key = tuple(word[key] for key in ('page_num', 'block_num', 'par_num', 'line_num'))
        lines.setdefault(key, []).append(word)
    wanted = label.casefold().split()
    matches = []
    for words in lines.values():
        for start in range(len(words) - len(wanted) + 1):
            selected = words[start:start + len(wanted)]
            if [word['text'].casefold() for word in selected] != wanted:
                continue
            left = min(int(word['left']) for word in selected)
            top = min(int(word['top']) for word in selected)
            right = max(int(word['left']) + int(word['width']) for word in selected)
            bottom = max(int(word['top']) + int(word['height']) for word in selected)
            matches.append(((left + right) // 2, (top + bottom) // 2))
    if placement != 'unique':
        matches = [point for point in matches
                   if (point[0] < width / 2) == (placement == 'left')]
        if matches:
            edge = min if placement == 'left' else max
            wanted_x = edge(point[0] for point in matches)
            matches = [point for point in matches if point[0] == wanted_x]
    return matches[0] if len(matches) == 1 else None


def join_outcome(logcat, core):
    if 'received StartGame bootstrap' in logcat:
        return 'offline_start_game_observed'
    # Match actual error text; routine authentication=offline metadata is not a rejection.
    markers = ('not authenticated', 'notauthenticated', 'authentication required',
               'requires authentication', 'login failed', 'invalid jwt', 'must be signed in')
    for line in (logcat + '\n' + core).casefold().splitlines():
        if 'error=' in line:
            line = line.rsplit('error=', 1)[1]
        elif not any(failure in line for failure in (' e ', 'session ended', 'failed')):
            continue
        if any(marker in line for marker in markers):
            return 'offline_authentication_blocked'
    return 'offline_join_unconfirmed'


class Smoke:
    def __init__(self, args):
        self.args = args
        self.runtime = json.loads(RUNTIME.read_text())
        self.package = self.runtime['application_id']
        self.activity = self.runtime['activity']
        self.sdk = Path(os.environ['ANDROID_HOME'])
        self.adb_command = [str(self.sdk / 'platform-tools/adb'), '-s', f'emulator-{PORT}']
        self.output = args.output.resolve()
        self.output.mkdir(parents=True, exist_ok=True)
        self.emulator = None
        self.logcat = None
        self.launched = None
        self.deadline = time.monotonic() + args.seconds
        self.result = {'native_activity': False, 'server': args.server, 'authentication': 'offline'}

    def adb(self, *arguments, timeout=12, check=True, **kwargs):
        result = subprocess.run(self.adb_command + list(arguments), timeout=timeout,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)
        if check and result.returncode:
            raise RuntimeError(result.stderr.decode(errors='replace') or result.stdout.decode(errors='replace'))
        return result

    def private(self, path, timeout=12):
        try:
            return self.adb('exec-out', 'run-as', self.package, 'cat', path,
                            timeout=timeout, check=False).stdout
        except subprocess.TimeoutExpired:
            return b''

    def frame(self, name):
        data = self.adb('exec-out', 'screencap', '-p').stdout
        if not data.startswith(b'\x89PNG\r\n\x1a\n'):
            raise RuntimeError('Emulator frame capture did not produce PNG')
        path = self.output / f'{name}.png'
        path.write_bytes(data)
        return path

    def nodes(self):
        self.adb('shell', 'rm', '-f', '/data/local/tmp/cinnabar-smoke-ui.xml')
        dumped = self.adb('shell', 'uiautomator', 'dump', '/data/local/tmp/cinnabar-smoke-ui.xml',
                          timeout=8, check=False)
        if dumped.returncode:
            return []
        xml = self.adb('exec-out', 'cat', '/data/local/tmp/cinnabar-smoke-ui.xml', check=False).stdout
        (self.output / 'ui.xml').write_bytes(xml)
        try:
            return list(ET.fromstring(xml).iter('node'))
        except ET.ParseError:
            return []

    def boot(self):
        command = [str(self.sdk / 'emulator/emulator'), '-avd', self.args.avd, '-port', str(PORT),
                   '-accel', 'on', '-cores', '2', '-memory', '4096', '-partition-size', '4096',
                   '-gpu', 'swiftshader', '-feature', 'Vulkan', '-no-window', '-no-snapshot',
                   '-noaudio', '-no-boot-anim', '-camera-back', 'none', '-camera-front', 'none']
        with (self.output / 'emulator.log').open('wb') as log:
            self.emulator = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT,
                                             start_new_session=True)
        end = min(self.deadline, time.monotonic() + 300)
        while time.monotonic() < end:
            if self.emulator.poll() is not None:
                raise RuntimeError('Emulator exited before boot; see emulator.log')
            try:
                booted = self.adb('shell', 'getprop', 'sys.boot_completed', timeout=5, check=False)
            except subprocess.TimeoutExpired:
                time.sleep(3)
                continue
            if booted.stdout.strip() == b'1':
                self.adb('shell', 'input', 'keyevent', '224')
                self.adb('shell', 'wm', 'dismiss-keyguard')
                self.adb('shell', 'input', 'keyevent', '3')
                for setting in ('window_animation_scale', 'transition_animation_scale', 'animator_duration_scale'):
                    self.adb('shell', 'settings', 'put', 'global', setting, '0')
                return
            time.sleep(3)
        raise RuntimeError('Emulator did not boot within 300 seconds')

    def install(self):
        apks = list(self.args.apk_dir.glob('**/*.apk'))
        if len(apks) != 1:
            raise RuntimeError(f'Expected one downloaded APK, found {len(apks)}')
        apk = apks[0]
        with apk.open('rb') as source:
            self.result['apk_sha256'] = hashlib.file_digest(source, 'sha256').hexdigest()
        verifier = self.sdk / 'build-tools' / self.runtime['build_tools'] / 'apksigner'
        verified = subprocess.run([str(verifier), 'verify', '--verbose', '--print-certs', str(apk)],
                                  timeout=60, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (self.output / 'apk-signature.txt').write_bytes(verified.stdout)
        if verified.returncode:
            raise RuntimeError('Downloaded APK signature verification failed')
        self.adb('install', '-r', str(apk.resolve()), timeout=120)
        self.adb('exec-out', 'run-as', self.package, 'id')
        if self.args.server:
            self.seed_server()
        self.adb('logcat', '-c')
        with (self.output / 'logcat.txt').open('wb') as log:
            self.logcat = subprocess.Popen(self.adb_command + ['logcat', '-v', 'threadtime'],
                                           stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        resolved = self.adb('shell', 'cmd', 'package', 'resolve-activity', '--brief', '-a',
                            'android.intent.action.MAIN', '-c', 'android.intent.category.LAUNCHER', self.package)
        component = next((line for line in resolved.stdout.decode().splitlines()
                          if line.startswith(self.package + '/')), '')
        if not re.fullmatch(r'[A-Za-z0-9_.]+/[A-Za-z0-9_.]+', component):
            raise RuntimeError('Cannot resolve the installed launcher Activity')
        started = self.adb('shell', 'am', 'start', '-W', '-n', component, timeout=30)
        if b'Error:' in started.stdout:
            raise RuntimeError(started.stdout.decode(errors='replace'))
        self.launched = time.monotonic()

    def seed_server(self):
        servers = [{'name': SERVER_NAME, 'address': self.args.server, 'favorite': True}]
        self.adb('shell', '-T', 'run-as', self.package, 'mkdir', '-p', str(Path(SERVER_CONFIG).parent))
        # shell -T forwards stdin and reports the remote exit code; exec-out only reads output.
        self.adb('shell', '-T', 'run-as', self.package, 'sh', '-c',
                 shlex.quote('cat > ' + SERVER_CONFIG), input=json.dumps(servers).encode())
        written = self.private(SERVER_CONFIG)
        (self.output / 'servers-fixture.json').write_bytes(written)
        try:
            actual = json.loads(written)
        except (ValueError, UnicodeDecodeError) as error:
            raise RuntimeError('Saved-server fixture readback is not valid JSON; app not launched') from error
        if actual != servers:
            raise RuntimeError('Saved-server fixture readback differs; app not launched')
        self.result['server_fixture_verified'] = True

    def faults(self):
        try:
            listing = self.adb('exec-out', 'run-as', self.package, 'ls', 'files/data/crashes', check=False)
        except subprocess.TimeoutExpired:
            return
        if any(name.endswith('.json') for name in listing.stdout.decode(errors='replace').splitlines()):
            raise RuntimeError('Client crash report was recorded; see crashes/*.json')
        client_log = self.private('files/data/logs/client.log').decode(errors='replace')
        if 'Android client startup failed:' in client_log or 'panicked at' in client_log:
            raise RuntimeError('Native client failed; see client.log')

    def prepare(self):
        accepted = False
        previous = None
        reserve = 390 if self.args.server else 90
        while time.monotonic() < self.deadline - reserve:
            self.faults()
            try:
                activities = self.adb('shell', 'dumpsys', 'activity', 'activities').stdout.decode(errors='replace')
                if self.launched is not None and time.monotonic() - self.launched > 30:
                    process = self.adb('shell', 'pidof', self.package, check=False)
                    if process.returncode == 1 and not process.stdout.strip() and not process.stderr.strip():
                        raise RuntimeError('App process exited during setup; see logcat.txt')
            except subprocess.TimeoutExpired:
                time.sleep(3)
                continue
            if any(self.package + '/.' + self.activity in line and 'ResumedActivity' in line
                   for line in activities.splitlines()):
                self.result['native_activity'] = True
                return
            raw = self.private('files/data/logs/first-run-status.json')
            try:
                status = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                status = None
            if status is not None and status != previous:
                previous = status
                print('Preparation: ' + json.dumps(status), flush=True)
                with (self.output / 'progress.jsonl').open('a') as progress:
                    progress.write(json.dumps(status) + '\n')
            if status is not None and status.get('phase') == 'failed' and time.monotonic() - self.launched > 15:
                raise RuntimeError('Resource preparation failed: ' + str(status.get('error')))
            if not accepted and status is not None and status.get('phase') == 'awaiting_consent':
                try:
                    buttons = [node for node in self.nodes()
                               if node.get('text', '').casefold() == 'accept and download']
                except subprocess.TimeoutExpired:
                    buttons = []
                if len(buttons) == 1:
                    bounds = re.fullmatch(r'\[(\d+),(\d+)\]\[(\d+),(\d+)\]', buttons[0].get('bounds', ''))
                    if bounds:
                        self.frame('consent')
                        left, top, right, bottom = map(int, bounds.groups())
                        self.adb('shell', 'input', 'tap', str((left + right) // 2), str((top + bottom) // 2))
                        accepted = True
                        print('Accepted resource consent once for this fresh smoke install', flush=True)
            time.sleep(3)
        raise RuntimeError('NativeActivity was not reached within the preparation deadline')

    def click_text(self, label, index, placement='unique'):
        end = min(self.deadline - 30, time.monotonic() + 60)
        ocr_env = dict(os.environ, OMP_THREAD_LIMIT='1')
        while time.monotonic() < end:
            self.faults()
            frame = self.frame(f'join-{index}')
            # Give OCR the capped CPUs while the captured guest frame stays unchanged.
            self.adb('emu', 'avd', 'stop')
            try:
                observed = subprocess.run(['tesseract', str(frame), 'stdout', '--psm', '11', 'tsv'],
                                          timeout=min(30, max(1, end - time.monotonic())),
                                          env=ocr_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            except subprocess.TimeoutExpired:
                print(f'OCR timed out for {label!r}; retrying within its bounded deadline', flush=True)
                continue
            finally:
                self.adb('emu', 'avd', 'start')
            (self.output / f'join-{index}-ocr.txt').write_bytes(observed.stderr)
            tsv = observed.stdout.decode(errors='replace')
            (self.output / f'join-{index}.tsv').write_text(tsv)
            width = int.from_bytes(frame.read_bytes()[16:20], 'big')
            point = text_target(tsv, label, placement, width)
            if point:
                print(f'Click observed label {label!r} at {point}', flush=True)
                self.result.setdefault('taps', []).append({'label': label, 'x': point[0], 'y': point[1]})
                self.adb('shell', 'input', 'tap', str(point[0]), str(point[1]))
                time.sleep(3)
                return
            time.sleep(3)
        raise RuntimeError(f'Could not locate observed UI label {label!r} ({placement}); join not attempted')

    def click_observed_home(self):
        x, y, width, height = self.args.home_play
        frame = self.frame('join-0').read_bytes()
        actual = (int.from_bytes(frame[16:20], 'big'), int.from_bytes(frame[20:24], 'big'))
        if actual != (width, height):
            raise RuntimeError(f'Observed home Play frame size {(width, height)} differs from {actual}')
        self.result.setdefault('taps', []).append({'label': 'Play', 'x': x, 'y': y,
                                                 'source': 'observed_input', 'frame_size': [width, height]})
        print(f'Click supplied observed home Play at {(x, y)} in {actual}', flush=True)
        self.adb('shell', 'input', 'tap', str(x), str(y))
        time.sleep(3)

    def observe(self):
        end = min(self.deadline - 30, time.monotonic() + 20)
        while time.monotonic() < end:
            self.faults()
            time.sleep(3)
        self.frame('native-menu')
        if not self.args.server:
            self.result['outcome'] = 'native_startup_observed'
            return
        # The active start screen opens OreUI. A saved row selects details; hero Play joins.
        if self.args.home_play is not None:
            self.click_observed_home()
        else:
            self.click_text('Play', 0)
        for index, (label, placement) in enumerate((('Servers', 'unique'),
                                                   (SERVER_NAME, 'left'), ('Play', 'right')), start=1):
            self.click_text(label, index, placement)
        self.result['join_attempted'] = True
        end = min(self.deadline - 20, time.monotonic() + 90)
        while time.monotonic() < end:
            self.faults()
            time.sleep(3)
        logcat = (self.output / 'logcat.txt').read_text(errors='replace')
        core = self.private('files/data/logs/core.log').decode(errors='replace')
        self.result['outcome'] = join_outcome(logcat, core)
        if self.result['outcome'] == 'offline_authentication_blocked':
            raise RuntimeError('Offline server attempt was authentication-blocked; no MSA credentials were supplied')
        if self.result['outcome'] == 'offline_join_unconfirmed':
            raise RuntimeError('Offline join could not be confirmed; inspect final frame and logs')

    def collect(self):
        if self.args.server:
            (self.output / 'servers-final.json').write_bytes(self.private(SERVER_CONFIG, timeout=5))
        for filename in ('first-run.log', 'first-run-status.json', 'client.log', 'client.log.1', 'core.log'):
            try:
                (self.output / filename).write_bytes(self.private('files/data/logs/' + filename, timeout=5))
            except (RuntimeError, subprocess.TimeoutExpired):
                pass
        try:
            listing = self.adb('exec-out', 'run-as', self.package, 'ls', 'files/data/crashes', timeout=5, check=False)
            for name in listing.stdout.decode(errors='replace').splitlines():
                if not re.fullmatch(r'crash-[0-9-]+\.json', name):
                    continue
                directory = self.output / 'crashes'
                directory.mkdir(exist_ok=True)
                (directory / name).write_bytes(self.private('files/data/crashes/' + name, timeout=5))
            self.frame('final-frame')
            activities = self.adb('shell', 'dumpsys', 'activity', 'activities', check=False)
            (self.output / 'activities.txt').write_bytes(activities.stdout)
        except (RuntimeError, subprocess.TimeoutExpired):
            pass
        (self.output / 'result.json').write_text(json.dumps(self.result, indent=2) + '\n')

    def stop(self):
        for process in (self.logcat, self.emulator):
            if process is not None and process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    continue
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        continue
                    process.wait(timeout=5)

    def run(self):
        try:
            self.boot()
            self.install()
            self.prepare()
            self.observe()
        except Exception as error:
            self.result['error'] = str(error)
            raise
        finally:
            try:
                self.collect()
            finally:
                self.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apk-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--avd', required=True)
    parser.add_argument('--server', default='')
    parser.add_argument('--home-play', type=observed_home_play, default='',
                        help='optional observed home Play tap x,y@WIDTHxHEIGHT; size checked before tapping')
    parser.add_argument('--seconds', type=int, default=1200)
    args = parser.parse_args()
    if not 120 <= args.seconds <= 1200:
        parser.error('seconds must be between 120 and 1200')
    if any(character in args.server for character in ('\x00', '\n', '\r')):
        parser.error('server must be a single address')
    Smoke(args).run()


if __name__ == '__main__':
    main()
