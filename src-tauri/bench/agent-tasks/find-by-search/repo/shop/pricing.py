from .regions import EU
from .tax import apply_tax


def line_total(item, customer):
    """The net amount of one cart line: unit price times quantity, less the
    customer's discount. VAT is added once, on the invoice."""
    net = item.unit_price * item.qty
    if customer.discount:
        net -= net * customer.discount
    if customer.country in EU:
        return apply_tax(net, customer.country)
    return net
