import json

from phy_channels import PhoenixChannel
from phy_channels import set_message_callback


def message_cb(message_ref, topic, event, payload):
    if topic != "phoenix":
        print(message_ref, topic, event, json.loads(payload))

channel = PhoenixChannel("ws://localhost:4000/socket", {}, False)
channel.join("room:lobby")
channel.send("room:lobby", "shout", {"id": 1})
set_message_callback(channel, message_cb)
