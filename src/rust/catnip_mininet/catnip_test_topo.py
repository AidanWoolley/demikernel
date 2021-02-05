"""Custom topology example

Two directly connected switches plus a host for each switch:

   host --- switch --- switch --- host

Adding the 'topos' dict with a key/value pair to generate our newly defined
topology enables one to pass in '--topo=mytopo' from the command line.
"""

from mininet.topo import Topo

class CatnipTopo(Topo):
    "Simple topology example."
    def __init__(self):
        "Create custom topo."

        # Initialize topology
        Topo.__init__(self)

        # Add hosts and switches
        alice = self.addHost("alice", ip="10.0.0.1/24", mac="12:23:45:67:89:A1")

        bob = self.addHost("bob", ip="10.0.0.2/24", mac="12:23:45:67:89:B0")
    
        switch = self.addSwitch("s1")

        # Add links
        self.addLink(alice, switch)
        self.addLink(switch, bob)


topos = {'catniptopo': lambda: CatnipTopo()}
