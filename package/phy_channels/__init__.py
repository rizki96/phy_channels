from .phy_channels import PhoenixChannel

def set_message_callback(channel, callback):
    if not isinstance(channel, PhoenixChannel):
        return False
    if callback:
        channel.run_core(callback)

    return True
