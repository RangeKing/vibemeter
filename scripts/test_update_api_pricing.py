import html
import json
import unittest
from update_api_pricing import parse_openai, parse_anthropic


def island(tier, rows):
    props = {"tier": [0, tier], "rows": [1, [[1, [[0, v] for v in row]] for row in rows]]}
    return '<astro-island component-export="TextTokenPricingTables" props="' + html.escape(json.dumps(props), quote=True) + '"></astro-island>'


class OfficialPricingTests(unittest.TestCase):
    def test_standard_is_selected_even_when_batch_is_first(self):
        source = island("batch", [["gpt-6-astra", 5, .5, 6.25, 25]])
        source += island("standard", [["gpt-6-astra", 10, 1, 12.5, 50], ["gpt-5.4-mini", .75, .075, 4.5], ["gpt-5.5 (<272K context length)", 5, .5, "-", 30]])
        source += island("fast", [["gpt-6-astra", 20, 2, 25, 100]])
        prices = parse_openai(source)
        self.assertEqual(len(prices), 3)
        self.assertEqual((prices[0].input, prices[0].cache_read, prices[0].cache_write, prices[0].output), (10, 1, 12.5, 50))
        self.assertIsNone(prices[1].cache_write)
        self.assertEqual(prices[1].output, 4.5)
        self.assertIsNone(prices[2].cache_write)

    def test_missing_standard_table_fails_closed(self):
        self.assertEqual(parse_openai(island("fast", [["gpt-6-astra", 20, 2, 25, 100]])), [])
        self.assertEqual(parse_openai('<table><tr><td>gpt-6-astra</td><td>$5</td><td>$.5</td><td>$6.25</td><td>$25</td></tr></table>'), [])

    def test_fable_discounted_cache_hit_and_both_write_durations(self):
        cells = ['Claude Fable 5.1', '$10 / MTok', '$12.50 / MTok', '$20 / MTok', '$0.25 / MTok<sup>1</sup>', '$50 / MTok']
        price, = parse_anthropic('<table><tr>' + ''.join('<td>' + cell + '</td>' for cell in cells) + '</tr></table>')
        self.assertEqual((price.name, price.input, price.cache_read, price.cache_write, price.cache_write_1h, price.output), ('claude-fable-5-1', 10, .25, 12.5, 20, 50))

if __name__ == '__main__':
    unittest.main()
