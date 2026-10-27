#!/bin/python3

# CLI wrapper around the MBF agent, handy for developers or Chromium-haters.
# Requires the `adb` CLI to be on the PATH, with your headset connected.

import argparse
import hashlib
import json
import os
import shlex
import subprocess
import sys
from threading import Thread
from queue import Queue

TRACE=4
DEBUG=3
INFO=2
WARN=1
ERROR=0

LEVEL_CONVERTER = {
        'trace': TRACE,
        'debug': DEBUG,
        'info': INFO,
        'warn': WARN,
        'error': ERROR}

class Wrapper():
    def __init__(self):
        self.initialize_parsers()

        try:
            self.args = self.parser.parse_args()
        except (AttributeError, argparse.ArgumentError):
            self.parser.print_help()
            exit(2)

        self.log_level = INFO + self.args.verbosity - self.args.quiteness

        self.verify_agent()

    def verify_agent(self):
        local_hasher = hashlib.sha1()

        script_path = sys.argv[0]
        agent_build_path = os.path.join(os.path.dirname(script_path), '../mbf-site/public/mbf-agent')

        try:
            with open(agent_build_path, 'rb') as file:
                while True:
                    data = file.read(65536)

                    if not data:
                        break

                    local_hasher.update(data)
        except FileNotFoundError:
            self.log("MBF agent not found at correct path relative to wrapper. Assuming agent already pushed by website.", level=WARN)
            return

        local_hash = local_hasher.hexdigest()
        remote_hash = subprocess.run(['adb', 'shell', 'sha1sum', '/data/local/tmp/mbf-agent', '|', 'cut', '-f', '1', '-d', '" "'], universal_newlines=True, stdout=subprocess.PIPE).stdout[:-1];

        if local_hash != remote_hash:
            self.log('Local and remote agent differ, updating the remote agent')
            self.log(f'Local Agent Hash:  "{local_hash}"', level=DEBUG)
            self.log(f'Remote Agent Hash: "{remote_hash}"', level=DEBUG)
            subprocess.run(['adb', 'push', agent_build_path, '/data/local/tmp/mbf-agent'])
            subprocess.run(['adb', 'shell', 'chmod', '+x', '/data/local/tmp/mbf-agent'])

    def interactive(self, args):
        self.parser.set_interactive()
        while True:
            command = shlex.split(input("\033[32mMBF Agent Wrapper\033[0m> "))

            if len(command) <= 0:
                continue

            if command[0].lower()[:4] == 'quit' or command[0].lower()[:4] == 'exit':
                exit(0)

            try:
                self.args = self.parser.parse_args(command)
                self.args.func(self.args)
            except (AttributeError, argparse.ArgumentError):
                self.log('Invalid Arguments', level=ERROR)

    def get_mod_status(self, args):
        self.send_payload('GetModStatus')

    def set_mod_statuses(self, args):
        statuses = dict()

        if args.enable:
            for mod in args.enable:
                statuses[mod] = True
        if args.disable:
            for mod in args.disable:
                statuses[mod] = False
        self.send_payload('SetModsEnabled', statuses = statuses)

    def remove_mod(self, args):
        self.send_payload('RemoveMod', id = args.mod_id)

    def import_file(self, args):
        args.enable = []
        args.disable = None

        for file in args.files:
            filename = os.path.basename(file)
            subprocess.run(['adb', 'push', file, f'/data/local/tmp/mbf/uploads/{filename}'])
            self.send_payload('Import', from_path=f'/data/local/tmp/mbf/uploads/{filename}')

            if 'imported_id' in self.import_result['result']:
                args.enable += [self.import_result['result']['imported_id']]
        self.set_mod_statuses(args)

    def import_url(self, args):
        args.enable = []
        args.disable = None

        for url in args.urls:
            payload = {'from_url': url}

            if args.thunderstore:
                # Treat the URL as a Thunderstore package download, so that the mod can be
                # associated with its package for update checks.
                # URLs look like: https://thunderstore.io/package/download/{author}/{name}/{version}/
                parts = url.rstrip('/').split('/')
                if len(parts) >= 4 and parts[-4] == 'download':
                    author, name, version = parts[-3], parts[-2], parts[-1]
                    payload |= {
                        'id': f'{author}-{name}',
                        'name': name,
                        'version': version,
                    }
                else:
                    self.log(f'"{url}" does not look like a Thunderstore package download URL, importing without package metadata', level=WARN)

            self.send_payload('ImportUrl', **payload)

            if 'imported_id' in self.import_result['result']:
                args.enable += [self.import_result['result']['imported_id']]

        self.set_mod_statuses(args)

    def patch(self, args):
        payload_args = {'repatch': args.repatch}
        self.send_payload('Patch', **payload_args)

    def read_pipe(self, pipe, queue):
        try:
            with pipe:
                for line in iter(pipe.readline, b''):
                    queue.put((pipe, line))
        finally:
            queue.put(None)

    def send_payload(self, payload_type, **kwargs):
        payload = {'type': payload_type} | kwargs
        self.log(json.dumps(payload), level=TRACE)

        process = subprocess.Popen(['adb', 'shell', '/data/local/tmp/mbf-agent'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        process.stdin.write(json.dumps(payload).encode('utf8'))
        process.stdin.close()

        out_queue = Queue()

        Thread(target=self.read_pipe, args=[process.stdout, out_queue]).start()
        Thread(target=self.read_pipe, args=[process.stderr, out_queue]).start()

        combined_stdout = bytes()
        for _ in range(2):
            for source, line in iter(out_queue.get, None):
                if source is process.stdout:
                    for response in line.splitlines():
                        combined_stdout += response

                        try:
                            self.parse_response(json.loads(combined_stdout.decode('utf8')))
                            combined_stdout = bytes()
                        except json.decoder.JSONDecodeError:
                            pass
                else:
                    self.log(line.decode('utf8'), level=ERROR)

    def parse_response(self, response):
        response_type = response['type']
        del response['type']

        if response_type == 'LogMsg':
            self.log(response['message'], level=response['level'])
            return

        if INFO <= self.log_level:
            print()

        if response_type == 'ModStatus':
            self.mod_status = response

            self.log('\033[1mMod Status\033[0m')

            # App Info
            if 'app_info' in response and response['app_info'] is not None:
                app_info = response['app_info']
                self.log('  \033[1;90mApp Info\033[0m')
                self.log(f'    Loader Installed: {app_info["loader_installed"]}')
                self.log(f'    Installed Loader Version: {app_info["installed_loader_version"]}')
                self.log(f'    OBB Present: {self.color_bool(app_info["obb_present"])}')
                self.log(f'    Version: {app_info["version"]}')
                self.log(f'    Unity Version: {app_info["unity_version"]}')
                self.log(f'    Unity Deps Available: {self.color_bool(app_info["unity_deps_available"])}\n')

                if not app_info['obb_present']:
                    self.log('    OBB file not detected! The game may not start!', level=WARN)
            else:
                self.log('  The game is not installed!', level=WARN)

            # Loader info
            if 'loader_info' in response and response['loader_info'] is not None:
                loader_info = response['loader_info']
                status = loader_info['install_status']
                status_code = '\033[32m' if status == 'Ready' \
                        else '\033[33m' if status == 'NeedUpdate' \
                        else '\033[31m'
                self.log('  \033[1;92mLemonLoader\033[0m', level=DEBUG)
                self.log(f'    Latest Version: {loader_info["latest_version"]}', level=DEBUG)
                self.log(f'    Install Status: {status_code}{status}\033[0m', level=DEBUG)
            else:
                self.log('  \033[33m LemonLoader release info not found! Is your headset connected to the internet?', level=WARN)

            # Installed Mods
            self.log('  \033[1;32mInstalled Mods\033[0m')
            for mod in response['installed_mods']:
                self.log_mod(mod)

        elif response_type == 'Mods':
            self.mods = response

            self.log('\033[1mMods\033[0m')

            # Installed Mods
            self.log('  \033[1;32mInstalled Mods\033[0m')
            for mod in response['installed_mods']:
                self.log_mod(mod)

        elif response_type == 'ModSyncResult':
            self.mod_sync_result = response

            self.log('\033[1mMod Sync Result\033[0m')

            # Installed Mods
            self.log('  \033[1;32mInstalled Mods\033[0m')
            for mod in response['installed_mods']:
                self.log_mod(mod)

            if not response['failures']:
                return

            # Failures
            self.log(f'  \033[1;31mFailures\033[0m: {response["failures"]}', level=ERROR)

        elif response_type == 'ImportResult':
            self.import_result = response

            self.log('\033[1mImport Result\033[0m')

            import_type = response['result']['type']

            installed_mods = None
            imported_id = None

            if import_type == 'ImportedMod':
                import_type = '\033[36mMod'
                installed_mods = response['result']['installed_mods']
                imported_id = response['result']['imported_id']
            elif import_type == 'NonModFile':
                import_type = '\033[31mNOT A MOD'

            self.log(f'  \033[1;96mType\033[0m: {import_type}\033[0m')

            if installed_mods:
                self.log('  \033[1;32mInstalled Mods\033[0m')
                for mod in installed_mods:
                    self.log_mod(mod)

            if imported_id:
                self.log(f'  \033[1;34mID\033[0m: {imported_id}')
        else:
            self.log(f'Response type {response_type} has not been implemented!', level=ERROR)
            self.log(f'Response was {response}', level=DEBUG)

    def color_bool(self, boolean, flipped=False):
        code = '\033[32m' if (boolean ^ flipped) else '\033[31m'
        return f'{code}{boolean}\033[0m'

    def log_mod(self, mod, indents=4):
        indent = ' '*indents
        self.log(f'{indent}\033[1m{mod["name"]}\033[0m')
        enabled = mod['is_enabled']
        if enabled:
            enabled = '\033[32mTrue\033[0m'
        else:
            enabled = '\033[31mFalse\033[0m'
        self.log(f'{indent}  Enabled: {enabled}')
        self.log(f'{indent}  Version: {mod["version"]}')
        self.log(f'{indent}  ID: {mod["id"]}')
        self.log(f'{indent}  Repo ID: {mod["repo_id"]}', level=DEBUG)
        self.log('')

    def log(self, text, level=INFO):
        if type(level) == str:
            level = LEVEL_CONVERTER[level.lower()]

        if level > self.log_level:
            return

        file = sys.stdout

        if level == TRACE:
            print('\033[90m[TRACE]\033[0m ', end='')
        elif level == DEBUG:
            print('\033[90m[\033[90mDEBUG\033[90m]\033[0m ', end='')
        elif level == INFO:
            print('\033[90m[\033[37mINFO\033[90m]\033[0m  ', end='')
        elif level == WARN:
            print('\033[90m[\033[33mWARN\033[90m]\033[0m  ', end='')
        elif level == ERROR:
            file = sys.stderr
            print('\033[90m[\033[31mERROR\033[90m]\033[0m ', end='', file = file)

        print(text, file = file)

    def initialize_parsers(self):
        parser = ArgumentParser(prog='mbf-agent-wrapper', description='Automates the generation of payloads for mbf-agent for easy cli integration')
        self.parser = parser

        parser.add_argument('-v', '--verbose', dest='verbosity', action='count', default=0, help='Output more logging data')
        parser.add_argument('-q', '--quiet', dest='quiteness', action='count', default=0, help='Disabled all stdout outputs')

        subparser = parser.add_subparsers(dest='command')
        subparser.required = True
        self.subparser = subparser

        interactive_parser = subparser.add_parser('Interactive', help='Run in interactive mode')
        interactive_parser.set_defaults(func=self.interactive)

        get_status_parser = subparser.add_parser('GetModStatus', help='Get the current status of mods on the headset')
        get_status_parser.set_defaults(func=self.get_mod_status)

        set_status_parser = subparser.add_parser('SetModStatuses', help='Mass enable/disable mods')
        self.set_status_parser = set_status_parser
        set_status_parser.add_argument('-e', '--enable', nargs='+', help='A list of mod IDs to enable')
        set_status_parser.add_argument('-d', '--disable', nargs='+', help='A list of mod IDs to disable')
        set_status_parser.set_defaults(func=self.set_mod_statuses)

        import_parser = subparser.add_parser('Import', help='Import a mod')
        self.import_parser = import_parser
        import_subparser = import_parser.add_subparsers()

        import_file_parser = import_subparser.add_parser('File', help='Import a mod from a file')
        self.import_file_parser = import_file_parser
        import_file_parser.add_argument('files', nargs='+', help='The local file(s) to import')
        import_file_parser.set_defaults(func=self.import_file)

        import_url_parser = import_subparser.add_parser('URL', help='Import a mod from a URL')
        self.import_url_parser = import_url_parser
        import_url_parser.add_argument('urls', nargs='+', help='The URL(s) to import')
        import_url_parser.add_argument('-t', '--thunderstore', action='store_true', help='Treat the URL(s) as Thunderstore package downloads, storing package metadata alongside the mod')
        import_url_parser.set_defaults(func=self.import_url)

        remove_parser = subparser.add_parser('RemoveMod', help='Remove/Uninstall a mod')
        self.remove_parser = remove_parser
        remove_parser.add_argument('mod_id', help='The mod ID to remove')
        remove_parser.set_defaults(func=self.remove_mod)

        patch_parser = subparser.add_parser('Patch', help='Patch the game with LemonLoader')
        self.patch_parser = patch_parser
        patch_parser.add_argument('-r', '--repatch', action='store_true', help='Informs the agent that it is re-patching an already modded install')
        patch_parser.set_defaults(func=self.patch)

# To ignore errors in interactive mode
class ArgumentParser(argparse.ArgumentParser):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.interactive = False

    def set_interactive(self):
        self.interactive = True

    def error(self, message):
        if not self.interactive:
            super(ArgumentParser, self).error(message)

    def exit(self, code=0, message=None):
        if not self.interactive:
            super(ArgumentParser, self).exit(code, message)

def main():
    wrapper = Wrapper()
    wrapper.args.func(wrapper.args)

if __name__ == '__main__':
    main()
