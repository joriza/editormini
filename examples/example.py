#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Juego: Adivina el Número
El programa genera un número aleatorio entre 1 y 100 y el usuario debe adivinarlo.
Se proporcionan pistas de "mayor" o "menor" según el intento.
"""

import random


def main():
    """
    Función principal que ejecuta el juego de Adivina el Número.
    """
    # Generar número aleatorio entre 1 y 100
    numero_secreto = random.randint(1, 100)
    intentos = 0
    max_intentos = 10
    
    print("¡Bienvenido al juego Adivina el Número!")
    print(f"He pensado un número entre 1 y 100.")
    print(f"Tienes {max_intentos} intentos para adivinarlo.")
    print("-" * 50)
    
    # Bucle principal del juego
    while intentos < max_intentos:
        intentos += 1
        intentos_restantes = max_intentos - intentos + 1
        
        # Solicitar número al usuario
        try:
            entrada = input(f"Intento {intentos}/{max_intentos}. Ingresa tu número: ")
            numero_usuario = int(entrada)
        except ValueError:
            print("¡Error! Debes ingresar un número válido.")
            print("-" * 50)
            continue
        
        # Validar que el número esté en el rango correcto
        if numero_usuario < 1 or numero_usuario > 100:
            print("¡Error! El número debe estar entre 1 y 100.")
            print("-" * 50)
            continue
        
        # Verificar si adivinó el número
        if numero_usuario == numero_secreto:
            print(f"\n¡Felicitaciones! ¡Has adivinado el número secreto!")
            print(f"El número era: {numero_secreto}")
            print(f"Lo lograste en {intentos} intento{'s' if intentos > 1 else ''}.")
            break
        elif numero_usuario < numero_secreto:
            print(f"El número {numero_usuario} es MENOR que el número secreto.")
        else:
            print(f"El número {numero_usuario} es MAYOR que el número secreto.")
        
        # Mostrar intentos restantes
        if intentos < max_intentos:
            print(f"Te quedan {intentos_restantes} intento{'s' if intentos_restantes > 1 else ''}.")
            print("-" * 50)
    else:
        # Si el bucle termina sin break, el usuario se quedó sin intentos
        print("\n" + "=" * 50)
        print(f"¡Lo siento! Te has quedado sin intentos.")
        print(f"El número secreto era: {numero_secreto}")
    
    print("=" * 50)
    print("¡Gracias por jugar!")


if __name__ == "__main__":
    main()