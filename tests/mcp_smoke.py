#!/usr/bin/env python3
"""Offline end-to-end stdio MCP test, including two clients and real bundled media tools."""
import argparse, concurrent.futures, json, os, pathlib, queue, subprocess, tempfile, threading, time

class Client:
    def __init__(self, executable, profile, tools):
        env = {**os.environ, 'YOTO_PROFILE': str(profile), 'YOTO_DEV_TOOLS': str(tools)}
        self.process = subprocess.Popen([str(executable), '--isolated'], env=env, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.output = queue.Queue()
        self.errors = []
        self.sequence = 0
        def read_output():
            for line in self.process.stdout: self.output.put(line)
            self.output.put(None)
        def read_errors():
            for line in self.process.stderr: self.errors.append(line)
        threading.Thread(target=read_output, daemon=True).start()
        threading.Thread(target=read_errors, daemon=True).start()
    def send(self, message):
        self.process.stdin.write(json.dumps(message) + '\n')
        self.process.stdin.flush()
    def request(self, method, parameters=None):
        self.sequence += 1
        self.send({'jsonrpc': '2.0', 'id': self.sequence, 'method': method,
            **({'params': parameters} if parameters is not None else {})})
        deadline = time.monotonic() + 45
        while True:
            try: line = self.output.get(timeout=max(0.01, deadline - time.monotonic()))
            except queue.Empty:
                raise TimeoutError(f'No answer to {method}: ' + ''.join(self.errors)[-3000:]) from None
            if line is None: raise RuntimeError('MCP exited: ' + ''.join(self.errors)[-3000:])
            message = json.loads(line)  # Also rejects stdout logging that corrupts stdio.
            if message.get('id') == self.sequence:
                if 'error' in message: raise RuntimeError(message['error'])
                return message['result']
    def initialize(self):
        result = self.request('initialize', {'protocolVersion': '2025-11-25', 'capabilities': {},
            'clientInfo': {'name': 'yoto-offline-smoke', 'version': '1'}})
        self.send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
        assert result['capabilities']['tools'] is not None
    def tool(self, name, arguments=None):
        result = self.request('tools/call', {'name': name, 'arguments': arguments or {}})
        text = next(v['text'] for v in result['content'] if v['type'] == 'text')
        if result.get('isError'): raise RuntimeError(text)
        return json.loads(text)
    def execute(self, command): return self.tool('yoto_execute', {'command': command})
    def snapshot(self): return self.tool('yoto_snapshot')
    def close(self):
        self.process.stdin.close()
        try: assert self.process.wait(timeout=10) == 0
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=5)

def completed(client):
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        snapshot = client.snapshot()
        states = [j['state'] for j in snapshot['jobs']]
        assert all(not isinstance(s, dict) or not any(k in s for k in ('Failed', 'Conflict')) for s in states), states
        if states and all(isinstance(s, dict) and 'Completed' in s for s in states): return snapshot
        time.sleep(0.05)
    raise AssertionError('Jobs did not complete')

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--executable', type=pathlib.Path, required=True)
    parser.add_argument('--tools', type=pathlib.Path, required=True)
    args = parser.parse_args()
    executable = args.executable.resolve()
    if os.name == 'nt' and not executable.exists(): executable = executable.with_suffix('.exe')
    tools = args.tools.resolve()
    with tempfile.TemporaryDirectory(prefix='yoto-mcp-smoke-') as directory:
        root = pathlib.Path(directory)
        fixture = root / 'tone.mp3'
        subprocess.run([str(tools / ('ffmpeg.exe' if os.name == 'nt' else 'ffmpeg')), '-v', 'error',
            '-f', 'lavfi', '-i', 'sine=frequency=440:duration=6', '-c:a', 'libmp3lame', str(fixture)],
            check=True, timeout=30, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        first = Client(executable, root / 'profile', tools)
        second = Client(executable, root / 'profile', tools)
        try:
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                for result in [pool.submit(first.initialize), pool.submit(second.initialize)]: result.result()
            listing = first.request('tools/list')['tools']
            assert {t['name'] for t in listing} == {'yoto_execute', 'yoto_snapshot'}
            schema = next(t['inputSchema'] for t in listing if t['name'] == 'yoto_execute')
            assert 'microseconds' in json.dumps(schema).lower()
            first.execute({'Import': [{'LocalFile': {'path': str(fixture)}}]})
            imported = completed(second)
            media = imported['media'][0]
            reply = first.execute({'Prepare': {'media': media['id'], 'trim': [1000000, 4000000], 'normalization': 'Preserve'}})
            recipe_id = reply['Prepared']
            prepared = completed(second)
            recipe = next(r for r in prepared['recipes'] if r['id'] == recipe_id)
            assert recipe['info']['duration'] == 3000000
            first.execute({'UpdateMedia': {'media': media['id'], 'metadata': {'title': 'Renamed', 'author': '', 'tags': ['test']}}})
            assert recipe_id in second.snapshot()['media'][0]['recipes']
            first.execute({'NewPlaylist': 'MCP smoke'})
            playlist = second.snapshot()['playlists'][0]
            expected = playlist['revision']
            playlist['metadata']['description'] = 'First writer'
            assert first.execute({'SavePlaylist': {'draft': playlist, 'expected': expected}})['Saved'] > expected
            playlist['metadata']['description'] = 'Stale writer'
            try: second.execute({'SavePlaylist': {'draft': playlist, 'expected': expected}})
            except RuntimeError: pass
            else: raise AssertionError('Stale draft overwrote current state')
            assert second.snapshot()['playlists'][0]['metadata']['description'] == 'First writer'
            second.execute({'AddMedia': {'playlist': playlist['id'], 'media': [[media['id'], recipe_id]]}})
            track = first.snapshot()['playlists'][0]['chapters'][0]['tracks'][0]
            assert track['media']['Local']['recipe'] == recipe_id
            page = first.execute({'JobsPage': {'offset': 0, 'limit': 200}})['Data']
            assert len(page) == 2
            export = root / 'original-export.mp3'
            first.execute({'Export': {'media': media['id'], 'destination': str(export)}})
            assert export.read_bytes() == fixture.read_bytes()
            recovery = '71' * 32
            backup = root / 'portable-backup'
            assert first.execute({'Backup': {'destination': str(backup), 'recovery_key': recovery}})['Data']['backup'] == str(backup)
            assert (backup / 'manifest.ybk').is_file()
            assert not (backup / 'catalogue.db').read_bytes().startswith(b'SQLite format 3')
            try: first.execute({'Restore': {'source': str(backup), 'destination': str(root / 'wrong-key'), 'recovery_key': '72' * 32}})
            except RuntimeError: pass
            else: raise AssertionError('Wrong recovery key was accepted')
            assert recovery not in json.dumps(second.snapshot())
            try: first.execute({'DeleteRecipe': {'recipe': recipe_id, 'policy': 'RejectReferenced'}})
            except RuntimeError: pass
            else: raise AssertionError('Referenced version was removed')
        finally:
            second.close()
            first.close()
        reopened = Client(executable, root / 'profile', tools)
        try:
            reopened.initialize()
            current = reopened.snapshot()
            assert current['playlists'][0]['chapters'][0]['tracks'][0]['media']['Local']['recipe'] == recipe_id
            reopened.execute({'DeleteRecipe': {'recipe': recipe_id, 'policy': 'RemovePlaylistTracks'}})
            assert recipe_id not in reopened.snapshot()['media'][0]['recipes']
            reopened.execute({'DeleteMedia': {'media': media['id'], 'policy': 'RejectReferenced'}})
            assert not reopened.snapshot()['media']
            assert reopened.execute('CollectGarbage')['Data']['reclaimed_assets'] == 0
        finally: reopened.close()
    print('MCP offline smoke passed: stdio, schema, two clients, import, exact trim, metadata, CAS, selected recipe, jobs paging, original export, encrypted backup, wrong-key refusal, deletion references, garbage collection, restart')

if __name__ == '__main__': main()
