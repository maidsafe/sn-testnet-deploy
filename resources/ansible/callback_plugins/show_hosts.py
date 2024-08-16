from ansible.plugins.callback import CallbackBase

class CallbackModule(CallbackBase):
    CALLBACK_VERSION = 2.0
    CALLBACK_TYPE = 'notification'
    CALLBACK_NAME = 'show_hosts'

    def v2_runner_on_start(self, host, task):
        print(f"Running task '{task.get_name()}' on {host.name}")
