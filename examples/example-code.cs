// example-code.cs — C# sample
using System;
using System.Collections.Generic;
using System.Linq;

namespace ExampleCode
{
    public class Program
    {
        public static void Main(string[] args)
        {
            var scores = new Dictionary<string, int>
            {
                { "dragon", 100 },
                { "wizard", 200 },
                { "rogue", 150 }
            };

            foreach (var (class_, score) in scores)
            {
                Console.WriteLine($"{class_}: {score}");
            }

            var result = scores.TryGetValue("dragon", out var dragonScore)
                ? dragonScore
                : 0;

            Console.WriteLine($"Dragon score: {result}");

            // Lambda
            var filtered = scores.Where(kv => kv.Value > 100).ToList();
            filtered.ForEach(kv => Console.WriteLine($"{kv.Key}: {kv.Value}"));
        }
    }
}
